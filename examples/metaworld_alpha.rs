//! Phase 1 Multiplayer Demo
//!
//! **FIRST PLAYABLE MULTIPLAYER DEMO** - Proves P2P architecture works end-to-end.
//!
//! Features:
//! - Two instances connect via P2P (mDNS discovery on localhost)
//! - Real-time player movement synchronization (20 Hz)
//! - Voxel dig/place operations sync with CRDT semantics
//! - Remote players rendered as blue wireframe capsules
//! - Chat messaging (T key to send)
//! - All Phase 1 features (walk/fly, physics, terrain interaction)
//! - **Persistent world state** - Edits save to disk, reload on restart
//!
//! # Usage
//!
//! **Single machine testing (3 terminals):**
//! ```bash
//! # Terminal 1 - Alice
//! METAVERSE_IDENTITY_FILE=~/.metaverse/alice.key cargo run --release --example phase1_multiplayer
//!
//! # Terminal 2 - Bob
//! METAVERSE_IDENTITY_FILE=~/.metaverse/bob.key cargo run --release --example phase1_multiplayer
//!
//! # Terminal 3 - Charlie
//! METAVERSE_IDENTITY_FILE=~/.metaverse/charlie.key cargo run --release --example phase1_multiplayer
//! ```
//!
//! **Or use --temp-identity for random keys (testing only):**
//! ```bash
//! cargo run --release --example phase1_multiplayer -- --temp-identity
//! ```
//!
//! All instances will auto-discover each other via mDNS within 1-2 seconds.
//! Move around in one window, see your player move in the other windows.
//! Dig/place blocks - changes appear in all connected clients.
//! **Close and restart - your edits persist!**
//!
//! # Persistence
//!
//! World state saved to `world_data/operations.json`:
//! - All voxel operations logged (dig, place)
//! - Automatically saved on exit
//! - Automatically loaded on startup
//! - Deterministic replay reconstructs exact state
//!
//! # Controls
//!
//! **Movement:**
//! - WASD - Move
//! - Space - Jump (walk mode) / Fly up (fly mode)
//! - Shift - Fly down (fly mode only)
//! - F - Toggle Walk/Fly mode
//!
//! **Interaction:**
//! - E - Dig voxel (10m reach)
//! - Q - Place stone voxel (10m reach)
//! - Mouse - Look around (click window to grab)
//! - ESC - Release mouse
//!
//! **Multiplayer:**
//! - T - Send test chat message
//! - Remote players appear as blue wireframe capsules
//! - Your name tag: Green capsule
//! - Remote name tags: Blue capsules with first 8 chars of PeerId
//!
//! **Debug:**
//! - F12 - Take screenshot
//! - Console shows connection events and sync statistics

use metaverse_core::{
    chunk::ChunkId,
    chunk_manager::ChunkManager,
    chunk_streaming::{ChunkStreamer, ChunkStreamerConfig},
    construct::{ConstructScene, SIGNUP_TERMINAL_POS, WORLD_PORTAL_POS, INTERACT_RADIUS,
                MODULE_DOOR_RADIUS, MODULES},
    billboard::{BillboardPipeline, ModuleBillboards, TerminalScreen},
    coordinates::{GPS, ECEF},
    elevation::{ElevationPipeline, OpenTopographySource},
    identity::{Identity, KeyType},
    marching_cubes::{extract_chunk_mesh, extract_water_surface_mesh},
    materials::MaterialId,
    mesh::{Mesh, Triangle, Vertex},
    messages::{Material, MovementMode},
    multiplayer::MultiplayerSystem,
    physics::{PhysicsWorld, Player, PHYSICS_TIMESTEP},
    player_persistence::PlayerPersistence,
    remote_render::{create_remote_player_capsule, remote_player_transform, short_peer_id},
    renderer::{Camera, GlbModel, MeshBuffer, OsmPipeline, RenderContext, RenderPipeline, TexturedPipeline},
    meshsite::Section,
    terrain::TerrainGenerator,
    user_content::UserContentLayer,
    vector_clock::VectorClock,
    voxel::VoxelCoord,
};
use egui_wgpu::ScreenDescriptor;
use glam::{Mat4, Vec3};
use rapier3d::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{EventLoop, ActiveEventLoop},
    keyboard::{KeyCode, PhysicalKey},
};
use std::sync::Mutex;

// Minimal ApplicationHandler wrapper — defers window creation to resumed(), then
// dispatches all events to the existing game-loop closure unchanged.
type GameHandlerFn = Box<dyn FnMut(Event<()>, &ActiveEventLoop)>;
type InitFn = Box<dyn FnOnce(&ActiveEventLoop) -> GameHandlerFn>;
struct GameApp { init: Option<InitFn>, handler: Option<GameHandlerFn> }
impl ApplicationHandler for GameApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if let Some(f) = self.init.take() { self.handler = Some(f(el)); }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, window_id: winit::window::WindowId, event: WindowEvent) {
        if let Some(h) = &mut self.handler { h(Event::WindowEvent { window_id, event }, el); }
    }
    fn device_event(&mut self, el: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
        if let Some(h) = &mut self.handler { h(Event::DeviceEvent { device_id, event }, el); }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(h) = &mut self.handler { h(Event::AboutToWait, el); }
    }
}

// ── Game mode — Construct (bundled lobby) vs Open World ───────────────────────
#[derive(Debug, Clone, PartialEq)]
enum GameMode {
    /// Player is in the bundled Construct lobby.
    /// Terrain streaming is paused; only construct geometry renders.
    Construct,
    /// Player has entered the open world through the portal.
    /// Construct geometry is hidden; terrain streams normally.
    OpenWorld,
}

// ── Signup screen ─────────────────────────────────────────────────────────────

enum SignupStep {
    Choosing,
    /// New User: choose display name
    CreateUser { name: String },
    /// New Guest: email + nickname
    CreateGuest { email: String, nick: String },
    /// Returning user: path to key file
    LoadKey { path: String, error: Option<String> },
}

struct SignupScreen {
    step: SignupStep,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

impl SignupScreen {
    fn new(context: &RenderContext, window: &winit::window::Window) -> Self {
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui_ctx.viewport_id(),
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &context.device,
            context.config.format,
            None,
            1,
            false,
        );
        Self { step: SignupStep::Choosing, egui_ctx, egui_state, egui_renderer }
    }

    /// Feed a window event to egui. Returns true if egui consumed the event.
    fn on_event(&mut self, window: &winit::window::Window, event: &WindowEvent) -> bool {
        self.egui_state.on_window_event(window, event).consumed
    }

    /// Render the signup overlay into an already-created texture view.
    /// Returns the user's choice once confirmed.
    fn render(
        &mut self,
        context: &RenderContext,
        view: &wgpu::TextureView,
        window: &winit::window::Window,
    ) -> Option<(KeyType, Option<String>, Option<String>)> {
        let raw_input = self.egui_state.take_egui_input(window);

        // Step transition flags — set inside egui closure, applied after.
        let mut result: Option<(KeyType, Option<String>, Option<String>)> = None;
        let mut next_step: Option<SignupStep> = None;

        let step = &mut self.step;
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::Area::new(egui::Id::new("signup_backdrop"))
                .fixed_pos(egui::pos2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        ctx.screen_rect(), 0.0,
                        egui::Color32::from_black_alpha(210),
                    );
                });

            egui::Window::new("Welcome to the Metaverse")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([500.0, 420.0])
                .show(ctx, |ui| {
                    ui.add_space(4.0);

                    match step {
                        SignupStep::Choosing => {
                            ui.label("You're in the lobby. Choose how to continue:");
                            ui.add_space(10.0);

                            // ── Returning user: load existing key ─────────────────────
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("  Load My Key  ").clicked() {
                                        next_step = Some(SignupStep::LoadKey {
                                            path: "~/.metaverse/identity.key".to_string(),
                                            error: None,
                                        });
                                    }
                                    ui.vertical(|ui| {
                                        ui.strong("Returning player");
                                        ui.small("Point to your identity.key file to sign in.");
                                    });
                                });
                            });

                            ui.add_space(8.0);
                            ui.separator();
                            ui.small("─── New here? ───");
                            ui.add_space(6.0);

                            // ── Trial: one-click, hourly reset ─────────────────────────
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("   Try It Now   ").clicked() {
                                        result = Some((KeyType::Trial, None, None));
                                    }
                                    ui.vertical(|ui| {
                                        ui.strong("Trial  —  no registration");
                                        ui.small("Walk around and look. Pre-set chat only.\nKey resets every hour — you return to the lobby for a new one.");
                                    });
                                });
                            });
                            ui.add_space(6.0);

                            // ── Guest: free account, email + nickname ─────────────────
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    if ui.button(" Free Account  ").clicked() {
                                        next_step = Some(SignupStep::CreateGuest {
                                            email: String::new(),
                                            nick: String::new(),
                                        });
                                    }
                                    ui.vertical(|ui| {
                                        ui.strong("Guest Account  —  free, verified email");
                                        ui.small("Home plot, public chat, receive items.\nUpgrade to full User after 30 days good standing.");
                                    });
                                });
                            });
                            ui.add_space(6.0);

                            // ── User: full account (invite or 30-day Guest) ───────────
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("  Full Account  ").clicked() {
                                        next_step = Some(SignupStep::CreateUser {
                                            name: String::new(),
                                        });
                                    }
                                    ui.vertical(|ui| {
                                        ui.strong("User Account  —  full access");
                                        ui.small("All features unlocked. Requires 30 days as Guest\nin good standing, or an invite code from a current user.");
                                    });
                                });
                            });
                        }

                        SignupStep::LoadKey { path, error } => {
                            ui.label(egui::RichText::new("Load existing key").strong());
                            ui.small("Enter the path to your identity.key file:");
                            ui.add_space(4.0);
                            ui.text_edit_singleline(path);
                            if let Some(err) = error.as_deref() {
                                ui.colored_label(egui::Color32::RED, err);
                            }
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button("  Load  ").clicked() {
                                    // Caller will handle file loading — signal via User type
                                    // with the path as the display_name field (reused for path).
                                    result = Some((KeyType::User, Some(path.clone()), None));
                                }
                                if ui.button("Back").clicked() {
                                    next_step = Some(SignupStep::Choosing);
                                }
                            });
                        }

                        SignupStep::CreateGuest { email, nick } => {
                            ui.label(egui::RichText::new("Create free Guest account").strong());
                            ui.add_space(8.0);
                            ui.label("Email address (required for verification):");
                            ui.text_edit_singleline(email);
                            ui.add_space(6.0);
                            ui.label("Nickname (how others see you):");
                            ui.text_edit_singleline(nick);
                            ui.add_space(10.0);
                            let can_create = !email.trim().is_empty() && !nick.trim().is_empty();
                            ui.horizontal(|ui| {
                                let btn = ui.add_enabled(can_create, egui::Button::new("  Create  "));
                                if btn.clicked() {
                                    result = Some((KeyType::Guest, Some(nick.trim().to_string()), Some(email.trim().to_string())));
                                }
                                if ui.button("Back").clicked() {
                                    next_step = Some(SignupStep::Choosing);
                                }
                            });
                            if !can_create {
                                ui.small("⚠  Both email and nickname are required.");
                            }
                        }

                        SignupStep::CreateUser { name } => {
                            ui.label(egui::RichText::new("Create full User account").strong());
                            ui.small("Requires 30 days as Guest in good standing, or an invite code.");
                            ui.add_space(8.0);
                            ui.label("Display name:");
                            ui.text_edit_singleline(name);
                            ui.add_space(6.0);
                            ui.label("Invite code (optional — reduces waiting period):");
                            // Invite code stored as email field in result for now
                            // TODO: wire into invite system when implemented
                            ui.add_space(10.0);
                            let can_create = !name.trim().is_empty();
                            ui.horizontal(|ui| {
                                let btn = ui.add_enabled(can_create, egui::Button::new("  Create  "));
                                if btn.clicked() {
                                    result = Some((KeyType::User, Some(name.trim().to_string()), None));
                                }
                                if ui.button("Back").clicked() {
                                    next_step = Some(SignupStep::Choosing);
                                }
                            });
                            if !can_create {
                                ui.small("⚠  A display name is required.");
                            }
                        }
                    }

                    ui.add_space(12.0);
                    ui.separator();
                    ui.small("⚠  Your key never leaves this machine. Back up your identity.key — there is no recovery.");
                });
        });

        if let Some(s) = next_step {
            self.step = s;
        }

        self.egui_state.handle_platform_output(window, full_output.platform_output);

        let tris = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&context.device, &context.queue, *id, delta);
        }
        let screen_desc = ScreenDescriptor {
            size_in_pixels: [context.config.width, context.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = context.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("egui_signup") }
        );
        self.egui_renderer.update_buffers(
            &context.device, &context.queue, &mut encoder, &tris, &screen_desc,
        );
        {
            // forget_lifetime() lets us hold a 'static RenderPass while
            // egui_renderer (which is 'static in the closure) renders into it.
            let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_signup_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // overlay on the existing 3D frame
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut rpass = rpass.forget_lifetime();
            self.egui_renderer.render(&mut rpass, &tris, &screen_desc);
        }
        context.queue.submit(std::iter::once(encoder.finish()));
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        result
    }
}

// ── Compose screen — in-game content posting ──────────────────────────────────

struct ComposeScreen {
    egui_ctx:      egui::Context,
    egui_state:    egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    section:       metaverse_core::meshsite::Section,
    title:         String,
    body:          String,
    author:        String,
    error:         Option<String>,
}

impl ComposeScreen {
    fn new(
        context: &RenderContext,
        window: &winit::window::Window,
        section: metaverse_core::meshsite::Section,
        author: String,
    ) -> Self {
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(), egui_ctx.viewport_id(), window,
            Some(window.scale_factor() as f32), None, None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &context.device, context.config.format, None, 1, false,
        );
        Self {
            egui_ctx, egui_state, egui_renderer,
            section, title: String::new(), body: String::new(),
            author, error: None,
        }
    }

    fn on_event(&mut self, window: &winit::window::Window, event: &WindowEvent) -> bool {
        self.egui_state.on_window_event(window, event).consumed
    }

    /// Render the compose overlay.
    /// Returns `Some(item)` when the user submits, `None` still composing, or drops self on cancel.
    /// The caller should set `compose = None` when this returns `Some(false_sentinel)`.
    fn render(
        &mut self,
        context: &RenderContext,
        view: &wgpu::TextureView,
        window: &winit::window::Window,
    ) -> ComposeResult {
        let raw_input = self.egui_state.take_egui_input(window);
        let mut result = ComposeResult::Continue;

        let section = &mut self.section;
        let title   = &mut self.title;
        let body    = &mut self.body;
        let error   = &mut self.error;

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            // dim backdrop
            egui::Area::new(egui::Id::new("compose_backdrop"))
                .fixed_pos(egui::pos2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.painter().rect_filled(ctx.screen_rect(), 0.0,
                        egui::Color32::from_black_alpha(190));
                });

            egui::Window::new("✍  New Post")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([520.0, 420.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Section:");
                        use metaverse_core::meshsite::Section;
                        for s in [Section::Forums, Section::Wiki, Section::Marketplace, Section::Post] {
                            let active = std::mem::discriminant(section) == std::mem::discriminant(&s);
                            let label = s.as_str();
                            if ui.selectable_label(active, label).clicked() {
                                *section = s;
                            }
                        }
                    });
                    ui.add_space(6.0);

                    ui.label("Title:");
                    ui.add(egui::TextEdit::singleline(title)
                        .desired_width(f32::INFINITY)
                        .hint_text("Subject / title (required)"));
                    ui.add_space(6.0);

                    ui.label("Body:");
                    ui.add(egui::TextEdit::multiline(body)
                        .desired_width(f32::INFINITY)
                        .desired_rows(10)
                        .hint_text("Write your post here..."));
                    ui.add_space(8.0);

                    if let Some(e) = error.as_deref() {
                        ui.colored_label(egui::Color32::from_rgb(255, 100, 100), e);
                        ui.add_space(4.0);
                    }

                    ui.horizontal(|ui| {
                        let can_submit = !title.trim().is_empty() && !body.trim().is_empty();
                        if ui.add_enabled(can_submit, egui::Button::new("📤  Post")).clicked() {
                            result = ComposeResult::Submit;
                        }
                        if ui.button("✖  Cancel").clicked() {
                            result = ComposeResult::Cancel;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format!("[ ESC to cancel ]"))
                                .color(egui::Color32::DARK_GRAY).size(11.0));
                        });
                    });
                });
        });

        self.egui_state.handle_platform_output(window, full_output.platform_output);
        let tris = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&context.device, &context.queue, *id, delta);
        }
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [context.config.width, context.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = context.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("compose") }
        );
        self.egui_renderer.update_buffers(&context.device, &context.queue, &mut encoder, &tris, &screen_desc);
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            }).forget_lifetime();
            self.egui_renderer.render(&mut rpass, &tris, &screen_desc);
        }
        context.queue.submit(std::iter::once(encoder.finish()));
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        result
    }
}

#[derive(PartialEq)]
enum ComposeResult {
    Continue,
    Submit,
    Cancel,
}

// ── In-game object placement overlay ─────────────────────────────────────────
// Press P to open. Shows a small centered panel to pick type and content key,
// then POSTs to the server API at the player's current look-ahead position.

struct PlacementScreen {
    egui_ctx:      egui::Context,
    egui_state:    egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    obj_type:      String,
    content_key:   String,
    label:         String,
    /// World position where the object will be placed (set on open).
    position:      [f32; 3],
    /// Player yaw at time of open — object faces toward player.
    rotation_y:    f32,
    placed_by:     String,
    status:        Option<String>,
}

impl PlacementScreen {
    fn new(
        context: &RenderContext,
        window: &winit::window::Window,
        position: [f32; 3],
        rotation_y: f32,
        placed_by: String,
    ) -> Self {
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(), egui_ctx.viewport_id(), window,
            Some(window.scale_factor() as f32), None, None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &context.device, context.config.format, None, 1, false,
        );
        Self {
            egui_ctx, egui_state, egui_renderer,
            obj_type: "Billboard".to_string(),
            content_key: "forums".to_string(),
            label: String::new(),
            position, rotation_y, placed_by,
            status: None,
        }
    }

    fn on_event(&mut self, window: &winit::window::Window, event: &WindowEvent) -> bool {
        self.egui_state.on_window_event(window, event).consumed
    }

    /// Returns `true` when the overlay should close (submitted or cancelled).
    fn render(
        &mut self,
        context: &RenderContext,
        view: &wgpu::TextureView,
        window: &winit::window::Window,
        server_url: &str,
    ) -> bool {
        let raw_input = self.egui_state.take_egui_input(window);
        let mut close = false;

        let obj_type    = &mut self.obj_type;
        let content_key = &mut self.content_key;
        let label       = &mut self.label;
        let status      = &mut self.status;
        let position    = self.position;
        let rotation_y  = self.rotation_y;
        let placed_by   = self.placed_by.clone();
        let server      = server_url.to_string();

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::Area::new(egui::Id::new("place_backdrop"))
                .fixed_pos(egui::pos2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.painter().rect_filled(ctx.screen_rect(), 0.0,
                        egui::Color32::from_black_alpha(160));
                });

            egui::Window::new("📌  Place Object")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([360.0, 240.0])
                .show(ctx, |ui| {
                    let accent = egui::Color32::from_rgb(0, 200, 160);

                    ui.horizontal(|ui| {
                        ui.label("Type:");
                        for t in ["Billboard", "Terminal", "Kiosk", "Portal", "SpawnPoint"] {
                            let sel = obj_type.as_str() == t;
                            if ui.selectable_label(sel, t).clicked() { *obj_type = t.to_string(); }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Content key:");
                        ui.text_edit_singleline(content_key);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Label (opt):");
                        ui.text_edit_singleline(label);
                    });
                    ui.add_space(4.0);
                    ui.colored_label(egui::Color32::GRAY, format!(
                        "Position: ({:.1}, {:.1}, {:.1})", position[0], position[1], position[2]));
                    ui.add_space(8.0);

                    if let Some(s) = status.as_deref() {
                        ui.colored_label(if s.starts_with("✓") { accent } else { egui::Color32::RED }, s);
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Place").clicked() && !content_key.is_empty() {
                            // Fire-and-forget POST
                            let body = serde_json::json!({
                                "id": "",
                                "object_type": *obj_type,
                                "position": position,
                                "rotation_y": rotation_y,
                                "scale": 1.0,
                                "content_key": *content_key,
                                "label": if label.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(label.clone()) },
                                "placed_by": placed_by,
                                "placed_at": 0u64,
                            });
                            let url = format!("{}/api/v1/world/objects", server);
                            let body_str = body.to_string();
                            std::thread::spawn(move || {
                                // blocking reqwest in a thread — simple and sufficient
                                let _ = reqwest::blocking::Client::new()
                                    .post(&url)
                                    .header("Content-Type","application/json")
                                    .body(body_str)
                                    .timeout(std::time::Duration::from_secs(5))
                                    .send();
                            });
                            *status = Some("✓ Placed — syncing to DHT…".to_string());
                            // close next frame
                        }
                        if ui.button("Cancel").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                            close = true;
                        }
                    });

                    // Auto-close after confirmed placement
                    if status.as_deref().map(|s| s.starts_with('✓')).unwrap_or(false) {
                        close = true;
                    }
                });
        });

        // Render egui into the frame
        let tris = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&context.device, &context.queue, *id, delta);
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [context.config.width, context.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = context.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("placement_overlay") });
        self.egui_renderer.update_buffers(&context.device, &context.queue, &mut encoder, &tris, &screen);
        {
            let rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("placement_overlay_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut rp = rp.forget_lifetime();
            self.egui_renderer.render(&mut rp, &tris, &screen);
        }
        context.queue.submit(std::iter::once(encoder.finish()));
        for id in &full_output.textures_delta.free { self.egui_renderer.free_texture(id); }
        self.egui_state.handle_platform_output(window, full_output.platform_output);

        close
    }
}

struct DebugHud {
    egui_ctx:      egui::Context,
    egui_state:    egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

impl DebugHud {
    fn new(context: &RenderContext, window: &winit::window::Window) -> Self {
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(), egui_ctx.viewport_id(), window,
            Some(window.scale_factor() as f32), None, None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            &context.device, context.config.format, None, 1, false,
        );
        Self { egui_ctx, egui_state, egui_renderer }
    }

    fn on_event(&mut self, window: &winit::window::Window, event: &WindowEvent) -> bool {
        self.egui_state.on_window_event(window, event).consumed
    }

    fn render(
        &mut self,
        context: &RenderContext,
        view: &wgpu::TextureView,
        window: &winit::window::Window,
        // Data to display
        game_mode: &str,
        gps: (f64, f64, f64),   // (lat, lon, alt_m)
        heading_rad: f32,        // camera yaw → compass bearing
        dist_portal: f32,
        dist_terminal: f32,
        near_portal: bool,
        near_terminal: bool,
        near_module: Option<usize>,
        // Road labels: (world_pos, name) — projected to screen space
        road_labels: &[(glam::Vec3, String)],
        view_proj: glam::Mat4,
    ) {
        // Convert yaw to compass bearing: yaw=0 → North, yaw=π/2 → West (wgpu -Z = north)
        // camera_yaw is rotation around Y; 0 = looking -Z (north), positive = clockwise
        let bearing_deg = ((-heading_rad.to_degrees()) % 360.0 + 360.0) % 360.0;
        let compass = match bearing_deg as u32 {
            0..=22   | 338..=360 => "N",
            23..=67              => "NE",
            68..=112             => "E",
            113..=157            => "SE",
            158..=202            => "S",
            203..=247            => "SW",
            248..=292            => "W",
            293..=337            => "NW",
            _                   => "N",
        };

        let raw_input = self.egui_state.take_egui_input(window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::Area::new(egui::Id::new("debug_hud"))
                .fixed_pos(egui::pos2(8.0, 8.0))
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::from_black_alpha(160))
                        .inner_margin(egui::Margin::same(6))
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(format!("Mode: {}", game_mode))
                                .color(egui::Color32::WHITE).size(13.0));
                            ui.label(egui::RichText::new(
                                format!("GPS  {:.5}°N  {:.5}°E", gps.0, gps.1))
                                .color(egui::Color32::from_rgb(120, 220, 255)).size(13.0).strong());
                            ui.label(egui::RichText::new(
                                format!("Alt: {:.1}m", gps.2))
                                .color(egui::Color32::LIGHT_GRAY).size(12.0));
                            ui.separator();

                            let portal_col = if near_portal { egui::Color32::from_rgb(80, 220, 255) }
                                             else { egui::Color32::GRAY };
                            ui.label(egui::RichText::new(
                                format!("Portal: {:.1}m{}", dist_portal,
                                    if near_portal { " ◀ WALK THROUGH" } else { "" }))
                                .color(portal_col).size(12.0));

                            let term_col = if near_terminal { egui::Color32::from_rgb(80, 255, 140) }
                                           else { egui::Color32::GRAY };
                            ui.label(egui::RichText::new(
                                format!("Terminal: {:.1}m{}", dist_terminal,
                                    if near_terminal { " ◀ PRESS E" } else { "" }))
                                .color(term_col).size(12.0));

                            if let Some(idx) = near_module {
                                let name = metaverse_core::construct::MODULES
                                    .get(idx).map(|m| m.name).unwrap_or("?");
                                ui.label(egui::RichText::new(
                                    format!("[ E ]  Enter {}", name))
                                    .color(egui::Color32::from_rgb(255, 220, 80))
                                    .size(13.0).strong());
                            }
                        });
                });

            // Compass rose — top-right corner
            {
                let compass_r = 30.0_f32;
                let sw_pts = ctx.screen_rect().width();
                let cx = sw_pts - compass_r - 14.0;
                let cy = compass_r + 14.0;
                let center = egui::pos2(cx, cy);
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground, egui::Id::new("compass_layer"),
                ));
                painter.circle_filled(center, compass_r + 3.0, egui::Color32::from_black_alpha(170));
                painter.circle_stroke(center, compass_r, egui::Stroke::new(1.5, egui::Color32::from_gray(120)));
                for i in 0..8 {
                    let a = ((i as f32) * 45.0 - bearing_deg).to_radians();
                    let inner = if i % 2 == 0 { compass_r - 6.0 } else { compass_r - 3.0 };
                    let p0 = center + egui::vec2(a.sin() * inner, -a.cos() * inner);
                    let p1 = center + egui::vec2(a.sin() * compass_r, -a.cos() * compass_r);
                    painter.line_segment([p0, p1], egui::Stroke::new(1.0, egui::Color32::from_gray(160)));
                }
                for (label, angle_offset) in [("N", 0.0_f32), ("E", 90.0_f32), ("S", 180.0_f32), ("W", 270.0_f32)] {
                    let a = (angle_offset - bearing_deg).to_radians();
                    let pos = center + egui::vec2(a.sin() * (compass_r - 10.0), -a.cos() * (compass_r - 10.0));
                    let color = if label == "N" { egui::Color32::from_rgb(230, 60, 60) }
                                else { egui::Color32::from_gray(220) };
                    painter.text(pos, egui::Align2::CENTER_CENTER, label,
                        egui::FontId::proportional(10.0), color);
                }
                // Forward arrow always pointing up (your facing direction)
                let tip = center + egui::vec2(0.0, -(compass_r - 6.0));
                let bl  = center + egui::vec2(-4.5, 4.0);
                let br  = center + egui::vec2( 4.5, 4.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![tip, bl, br],
                    egui::Color32::from_rgb(255, 210, 50),
                    egui::Stroke::NONE,
                ));
                painter.circle_filled(center, 3.0, egui::Color32::WHITE);
                painter.text(
                    egui::pos2(cx, cy + compass_r + 9.0),
                    egui::Align2::CENTER_CENTER,
                    format!("{}  {:.0}°", compass, bearing_deg),
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_gray(200),
                );
            }

            // Road name labels — projected from 3D world space to screen
            let sw = context.config.width as f32;
            let sh = context.config.height as f32;
            let ppp = ctx.pixels_per_point();
            for (world_pos, name) in road_labels {
                // Project world → clip space
                let clip = view_proj.project_point3(*world_pos);
                // clip.z <= 0 means behind camera
                if clip.z <= 0.0 || clip.z > 1.0 { continue; }
                // NDC to screen pixels
                let sx = (clip.x * 0.5 + 0.5) * sw;
                let sy = (1.0 - (clip.y * 0.5 + 0.5)) * sh;
                // Skip if outside viewport
                if sx < 0.0 || sx > sw || sy < 0.0 || sy > sh { continue; }
                let screen_pos = egui::pos2(sx / ppp, sy / ppp);
                egui::Area::new(egui::Id::new(format!("road_lbl_{}", name)))
                    .fixed_pos(screen_pos)
                    .show(ctx, |ui| {
                        ui.label(egui::RichText::new(name.as_str())
                            .color(egui::Color32::from_rgb(255, 255, 100))
                            .size(11.0)
                            .background_color(egui::Color32::from_black_alpha(120)));
                    });
            }
        });

        self.egui_state.handle_platform_output(window, full_output.platform_output);
        let tris = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&context.device, &context.queue, *id, delta);
        }
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [context.config.width, context.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = context.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("hud") }
        );
        self.egui_renderer.update_buffers(&context.device, &context.queue, &mut encoder, &tris, &screen_desc);
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hud_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            }).forget_lifetime();
            self.egui_renderer.render(&mut rpass, &tris, &screen_desc);
        }
        context.queue.submit(std::iter::once(encoder.finish()));
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlayerModeLocal {
    Walk,  // Physics-based, can walk/jump
    Fly,   // Free movement, no gravity
}

fn main() {
    env_logger::init();

    // ============================================================
    // ZONE CONFIGURATION
    // ============================================================
    // Toggle terrain editability for testing different zone types:
    //   true  = Editable zone (desert, quarry, beach)
    //   false = Protected zone (real-world terrain, infrastructure)
    //
    // Future: Replace with proper zone system based on GPS coordinates
    const TERRAIN_IS_EDITABLE: bool = true;
    
    if !TERRAIN_IS_EDITABLE {
        println!("⛔ PROTECTED ZONE - Terrain editing disabled");
        println!("   This represents real-world terrain (rivers, cliffs, etc.)");
        println!("   that cannot be modified in production.\n");
    }
    // ============================================================
    
    println!("=== Phase 1 Multiplayer Demo ===");
    println!();
    println!("🌐 P2P NETWORKING ENABLED");
    println!("   - Auto-discovery via mDNS (localhost)");
    println!("   - Player state sync @ 20 Hz");
    println!("   - Voxel operations with CRDT");
    println!("   - Ed25519 signatures");
    println!("   - World state persistence");
    println!();
    println!("Controls:");
    println!("  WASD - Move");
    println!("  Space - Jump (walk) / Up (fly)");
    println!("  Shift - Down (fly mode)");
    println!("  F - Toggle Walk/Fly mode");
    println!("  E - Dig voxel (10m reach)");
    println!("  Q - Place voxel (10m reach)");
    println!("  T - Send test chat message");
    println!("  Mouse - Look around (click to grab)");
    println!("  ESC - Release mouse");
    println!("  F12 - Take screenshot\n");
    
    // Initialize P2P networking
    println!("🔐 Initializing cryptographic identity...");

    // Detect first run before creating/loading the identity.
    // --temp-identity flag skips signup (used for multi-instance testing).
    let is_temp = std::env::args().any(|arg| arg == "--temp-identity");
    // --server http://192.168.1.x:8080  — server base URL for world object API
    let server_url: String = std::env::args()
        .skip_while(|a| a != "--server")
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
    let needs_signup = !is_temp && !Identity::key_file_exists();

    // Check for --temp-identity flag for testing multiple instances
    let identity = if is_temp {
        println!("   Using temporary identity (not saved)");
        Identity::generate()
    } else if needs_signup {
        // First run: generate in-memory, let the user choose type via UI.
        // The key will be saved to disk after the signup screen completes.
        println!("   First run — signup screen will appear in-game.");
        Identity::generate()
    } else {
        Identity::load_or_create()
            .expect("Failed to create identity")
    };

    println!("   PeerId: {}", short_peer_id(&identity.peer_id()));
    if !needs_signup { println!("   Key: ~/.metaverse/identity.key"); }
    
    println!("\n🌐 Starting P2P network node...");
    
    // Clone identity for multiplayer (we need it later for player persistence)
    let mut multiplayer = MultiplayerSystem::new_with_runtime(identity.clone())
        .expect("Failed to create multiplayer system");
    
    // Start listening on all available transports for maximum connectivity
    // TCP (primary transport) + QUIC (UDP-based, better NAT traversal)
    multiplayer.listen_on("/ip4/0.0.0.0/tcp/0")
        .expect("Failed to listen on TCP");
    multiplayer.listen_on("/ip4/0.0.0.0/udp/0/quic-v1")
        .expect("Failed to listen on QUIC");
    
    println!("📡 Multi-transport enabled: TCP + QUIC (universal connectivity)");
    
    // Connect to relay server for NAT traversal
    // Relay running on Android phone: 49.182.84.9:4001
    // Peer ID: 12D3KooWEzai1nEViFuX6JmLWDLU61db7T1A3hyd4xpmGs4W59ge
    let relay_addr = "/ip4/49.182.84.9/tcp/4001/p2p/12D3KooWEzai1nEViFuX6JmLWDLU61db7T1A3hyd4xpmGs4W59ge";
    println!("📡 Connecting to relay on phone: {}", relay_addr);
    if let Err(e) = multiplayer.dial(relay_addr) {
        println!("⚠️  Failed to connect to relay: {} (continuing without relay)", e);
    }
    
    println!("   Listening for connections...");
    println!("   mDNS discovery active (auto-connect on LAN)");
    println!("   PeerId: {}", multiplayer.peer_id());
    println!("\n⏳ Waiting for peers to connect...");
    println!("   (Watch for \"Peer discovered\" and \"Peer connected\" messages)");
    println!("   Note: Publishing will fail until at least one peer connects - this is normal!\n");
    println!();
    
    // Create window - sized for 4 instances on 1080p screen (960x540 each)
    let event_loop = EventLoop::new().unwrap();
    let mut app = GameApp {
        handler: None,
        init: Some(Box::new(move |el: &ActiveEventLoop| -> GameHandlerFn {
    let window = Arc::new(el.create_window(
        winit::window::WindowAttributes::default()
            .with_title("Phase 1 Multiplayer - Metaverse Core")
            .with_inner_size(winit::dpi::LogicalSize::new(960, 540))
    ).unwrap());
    
    // Initialize renderer
    println!("🎨 Initializing renderer...");
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let mut pipeline = RenderPipeline::new(&context);
    // OSM pipeline — flat vertex-colour shader for roads, buildings, water surface.
    // Treats the "normal" vertex slot as RGB so colours pass through without lighting.
    let osm_pipeline = OsmPipeline::new(
        &context,
        &pipeline.camera_bind_group_layout,
        &pipeline.model_bind_group_layout,
    );

    // Textured pipeline for GLB model rendering (buildings, world objects)
    let textured_pipeline = TexturedPipeline::new(
        &context,
        &pipeline.camera_bind_group_layout,
        &pipeline.model_bind_group_layout,
    );

    // Pre-load Kenney building GLB models — one per category
    // Indices: 0=commercial, 1=industrial, 2=residential, 3=skyscraper, 4=default
    let assets_base = "assets/models";
    let building_glb_paths = [
        format!("{}/kenney_city-kit-commercial_2.1/Models/GLB format/building-a.glb", assets_base),
        format!("{}/kenney_city-kit-industrial_1.0/Models/GLB format/building-a.glb", assets_base),
        format!("{}/kenney_city-kit-suburban_20/Models/GLB format/building-type-a.glb", assets_base),
        format!("{}/kenney_city-kit-commercial_2.1/Models/GLB format/building-skyscraper-a.glb", assets_base),
        format!("{}/kenney_building-kit/Models/GLB format/building-a.glb", assets_base),
    ];
    let building_glb_models: Vec<Option<GlbModel>> = building_glb_paths.iter().map(|p| {
        let model = textured_pipeline.load_glb(&context.device, &context.queue, p);
        if model.is_none() {
            println!("⚠️  Failed to load GLB model: {}", p);
        } else {
            println!("✅ Loaded GLB model: {}", p);
        }
        model
    }).collect();
    let glb_models_loaded = building_glb_models.iter().any(|m| m.is_some());
    println!("🏢 GLB building models loaded: {}/5", building_glb_models.iter().filter(|m| m.is_some()).count());

    // Billboard pipeline — renders textured quads on Construct module room walls
    let billboard_pipeline = BillboardPipeline::new(&context);
    let mut module_billboards: [Option<ModuleBillboards>; 6] = Default::default();
    let mut billboard_frame_counter = 0u32;
    // Placed world-object billboards: (object_id, built billboard). Rebuilt on cache change.
    let mut placed_billboards: Vec<(String, ModuleBillboards)> = Vec::new();

    // OSM-inferred geometry buffers (buildings, roads, water)
    let mut buildings_mesh_buffer: Option<MeshBuffer> = None;
    let mut roads_mesh_buffer: Option<MeshBuffer> = None;
    let mut water_mesh_buffer: Option<MeshBuffer> = None;
    // GLB building instances for textured rendering: (model_category_idx, model_bind_group)
    // model_category_idx: 0=commercial, 1=industrial, 2=residential, 3=skyscraper, 4=default
    let mut building_instances: Vec<(usize, wgpu::BindGroup)> = Vec::new();
    // Accumulated road segments for the visible area: (a, b, width, road_type)
    let mut road_segments: Vec<(Vec3, Vec3, f32, metaverse_core::osm::RoadType)> = Vec::new();
    // Named road midpoints for on-screen labels: (world_pos, name)
    let mut road_labels: Vec<(Vec3, String)> = Vec::new();
    // Accumulated water polygons: each entry is a list of local-space vertices
    let mut water_polygons: Vec<Vec<Vec3>> = Vec::new();
    // Deduplication: track first-vertex GPS (as ordered pair) to avoid adding same polygon twice
    let mut water_poly_seen: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    // Set to true whenever new objects are registered, triggering a mesh rebuild
    let mut osm_geom_dirty = false;
    // Debug: press O to toggle bright-yellow OSM water polygon outlines
    let mut show_osm_debug = false;
    let mut osm_debug_buffer: Option<MeshBuffer> = None;

    // WORLDNET terminal screen — rendered onto the kiosk top face
    let terminal_screen = TerminalScreen::new(&context, &billboard_pipeline, SIGNUP_TERMINAL_POS);
    let mut worldnet_buf = metaverse_core::worldnet::WorldnetPixelBuffer::new();
    // Render initial page (home, no key yet — will update each frame when near terminal)
    {
        use metaverse_core::worldnet::{WorldnetAddress, render_page};
        render_page(&WorldnetAddress::Root, None, &[], &mut worldnet_buf);
        terminal_screen.update(&context.queue, &worldnet_buf);
    }

    // Always-on debug HUD (top-left overlay)
    let mut hud = DebugHud::new(&context, &window);

    // First-run signup screen (shown when no identity key exists on disk)
    let mut signup: Option<SignupScreen> = if needs_signup {
        println!("🆕 First run detected — showing identity setup screen.");
        Some(SignupScreen::new(&context, &window))
    } else {
        None
    };

    // In-game compose screen (None when not composing)
    let mut compose: Option<ComposeScreen> = None;
    let mut placement: Option<PlacementScreen> = None;

    // Always start in the Construct; player enters Open World through the portal.
    let mut game_mode = GameMode::Construct;
    
    // Setup terrain generation with SRTM data
    println!("🗺️  Setting up chunk-based terrain generation...");
    
    let origin_gps = GPS::new(-27.463675, 153.035645, 10.0); // Story Bridge, Brisbane
    
    let mut elevation_pipeline = ElevationPipeline::new();
    
    // Standardise on OpenTopography API only — ensures all clients generate
    // identical terrain from the same source data. NAS file is excluded because
    // different SRTM datasets (NAS vs API) produce slightly different heights,
    // causing 1-2 block offsets between clients even at the same GPS coordinates.
    let data_dir = std::path::PathBuf::from("world_data");
    let cache_dir = data_dir.join("elevation_cache");
    let api_key = std::env::var("OPENTOPOGRAPHY_API_KEY").ok();
    if let Some(key) = api_key {
        elevation_pipeline.add_source(Box::new(OpenTopographySource::new(key, cache_dir)));
    } else {
        println!("⚠️  No OPENTOPOGRAPHY_API_KEY set — terrain will be flat");
    }
    
    // Convert GPS origin to voxel coordinates  
    let origin_ecef = origin_gps.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    
    println!("   Origin GPS: ({:.6}, {:.6}, {:.1}m)", origin_gps.lat, origin_gps.lon, origin_gps.alt);
    println!("   Origin voxel: {:?}", origin_voxel);
    
    // Create terrain generator with origin for coordinate conversion
    let elevation_pipeline_1 = elevation_pipeline;
    let generator = TerrainGenerator::new(elevation_pipeline_1, origin_gps, origin_voxel)
        .with_osm_cache(data_dir.join("osm"))
        .with_tile_fetcher(Arc::new(multiplayer.tile_fetcher()));
    let generator_arc = Arc::new(Mutex::new(generator));
    
    // Create second elevation pipeline for chunk_manager (same source as above)
    let mut elevation_pipeline_2 = ElevationPipeline::new();
    let cache_dir_2 = data_dir.join("elevation_cache");
    if let Some(key) = std::env::var("OPENTOPOGRAPHY_API_KEY").ok() {
        elevation_pipeline_2.add_source(Box::new(OpenTopographySource::new(key, cache_dir_2)));
    }
    let chunk_manager_generator = TerrainGenerator::new(elevation_pipeline_2, origin_gps, origin_voxel)
        .with_osm_cache(data_dir.join("osm"))
        .with_tile_fetcher(Arc::new(multiplayer.tile_fetcher()));

    // Third elevation pipeline for OSM inference (objects need ground-truth elevation)
    let mut osm_elev_pipeline = ElevationPipeline::new();
    let cache_dir_3 = data_dir.join("elevation_cache");
    if let Some(key) = std::env::var("OPENTOPOGRAPHY_API_KEY").ok() {
        osm_elev_pipeline.add_source(Box::new(OpenTopographySource::new(key, cache_dir_3)));
    }
    let osm_cache_dir = data_dir.join("osm");
    
    // User content layer - separates edits from base terrain
    let user_content = Arc::new(Mutex::new(UserContentLayer::new()));
    // Derive at-rest encryption key from identity signing key
    {
        let enc_key = UserContentLayer::derive_encryption_key(&identity.signing_key().to_bytes());
        user_content.lock().unwrap().set_encryption_key(enc_key);
    }
    // Advertise this client's capabilities to the DHT (0 = no storage contribution by default)
    multiplayer.publish_node_capabilities(0);
    
    // World data directory - single shared location
    let world_dir = std::path::PathBuf::from("world_data");
    
    // Create world directory if it doesn't exist
    if !world_dir.exists() {
        std::fs::create_dir_all(&world_dir).expect("Failed to create world data directory");
        println!("📁 Created world data directory: {:?}", world_dir);
    }

    // Load persisted voxel ops from disk into user_content so chunk_manager
    // can include them in state sync with reconnecting peers.
    {
        let mut uc = user_content.lock().unwrap();
        let chunks_dir = world_dir.join("chunks");
        if chunks_dir.exists() {
            // Discover all saved chunk dirs and load their ops
            let mut chunk_ids_to_load: Vec<metaverse_core::chunk::ChunkId> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&chunks_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    // chunk dir names are like "chunk_44753_44780_116080"
                    let parts: Vec<&str> = name_str.split('_').collect();
                    if parts.len() == 4 && parts[0] == "chunk" {
                        if let (Ok(x), Ok(y), Ok(z)) = (
                            parts[1].parse::<i64>(),
                            parts[2].parse::<i64>(),
                            parts[3].parse::<i64>(),
                        ) {
                            chunk_ids_to_load.push(metaverse_core::chunk::ChunkId { x, y, z });
                        }
                    }
                }
            }
            if !chunk_ids_to_load.is_empty() {
                match uc.load_chunks(&world_dir, &chunk_ids_to_load) {
                    Ok(counts) => {
                        let total: usize = counts.values().sum();
                        if total > 0 {
                            println!("📂 Loaded {} persisted voxel ops from {} chunks", total, counts.len());
                        }
                    }
                    Err(e) => eprintln!("⚠️  Failed to load persisted ops: {}", e),
                }
            }
        }
    }

    // Advertise all chunks we have on disk to the DHT so peers can find us as providers.
    // This runs after multiplayer is started but before the event loop — DHT bootstrap
    // will propagate the provider records once we connect to the relay.
    {
        let chunks_dir = world_dir.join("chunks");
        let mut startup_chunk_ids: Vec<metaverse_core::chunk::ChunkId> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&chunks_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                let parts: Vec<&str> = name_str.split('_').collect();
                if parts.len() == 4 && parts[0] == "chunk" {
                    if let (Ok(x), Ok(y), Ok(z)) = (
                        parts[1].parse::<i64>(),
                        parts[2].parse::<i64>(),
                        parts[3].parse::<i64>(),
                    ) {
                        startup_chunk_ids.push(metaverse_core::chunk::ChunkId { x, y, z });
                    }
                }
            }
        }
        if !startup_chunk_ids.is_empty() {
            println!("🗄️  Advertising {} local chunks to DHT", startup_chunk_ids.len());
            multiplayer.advertise_chunks(&startup_chunk_ids);
        }
    }
    println!("🔄 Initializing chunk streaming system...");
    let stream_config = ChunkStreamerConfig {
        load_radius_m: 150.0,           // 150m view distance
        unload_radius_m: 250.0,         // 100m hysteresis — no churn when walking
        max_loaded_chunks: 600,         // Headroom above ~300 max at 150m
        safe_zone_radius: 2,            // Keep 5×5 chunks around player (always loaded)
        frame_budget_ms: 5.0,
        max_in_flight: 16,              // Only 16 chunks dispatched to workers at once
    };
    let mut chunk_streamer = ChunkStreamer::new(stream_config, generator_arc.clone(), user_content.clone(), world_dir.clone());
    
    // Terrain chunks queued only when entering Open World — not needed in Construct.
    // chunk_streamer.update(spawn_ecef);  // deferred until portal transition
    
    // Keep chunk manager for user edits and voxel operations tracking only
    let chunk_manager_user_content = user_content.lock().unwrap().clone();
    let mut chunk_manager = ChunkManager::new(chunk_manager_generator, chunk_manager_user_content);

    // Initialize physics world (empty — terrain colliders added as chunks build in-loop)
    let origin_voxel_ecef = origin_voxel.to_ecef();
    let mut physics = PhysicsWorld::with_origin(origin_voxel_ecef);

    // ── Build the Construct scene ──────────────────────────────────────────────
    // The Construct is always available — floor, pillars, terminals, portal.
    // It loads from bundled geometry with no network or terrain dependency.
    println!("🏛️  Building construct scene...");
    let construct = ConstructScene::build();
    let construct_floor_buffer   = MeshBuffer::from_mesh(&context.device, &construct.floor);
    let construct_pillars_buffer = MeshBuffer::from_mesh(&context.device, &construct.pillars);
    let construct_terminal_buffer= MeshBuffer::from_mesh(&context.device, &construct.signup_terminal);
    let construct_portal_buffer  = MeshBuffer::from_mesh(&context.device, &construct.world_portal);
    let construct_doors_buffer   = MeshBuffer::from_mesh(&context.device, &construct.module_doors);

    // Add the construct floor as a static physics collider so the player
    // has ground to stand on from frame 1 — no terrain streaming needed.
    let floor_collision = metaverse_core::construct::build_floor_collision_mesh();
    metaverse_core::physics::create_collision_from_mesh(
        &mut physics, &floor_collision, &origin_voxel, None);
    println!("✅ Construct ready — floor collider active");

    // Create player model (visible cube) - green for local player
    let player_mesh = create_local_player_cube();
    let _player_model_buffer = MeshBuffer::from_mesh(&context.device, &player_mesh);
    
    // Create hitbox visualization
    let hitbox_mesh = create_hitbox_wireframe();
    let hitbox_buffer = MeshBuffer::from_mesh(&context.device, &hitbox_mesh);
    
    // Create crosshair
    let crosshair_mesh = create_crosshair();
    let crosshair_buffer = MeshBuffer::from_mesh(&context.device, &crosshair_mesh);
    
    // Create remote player mesh (blue wireframe) - reused for all remote players
    let remote_player_mesh = create_remote_player_capsule();
    let remote_player_buffer = MeshBuffer::from_mesh(&context.device, &remote_player_mesh);
    
    // ============================================================
    // PLAYER SETUP - Load last position or use default spawn
    // ============================================================
    
    // If no local save exists, request the session record from DHT so we can
    // resume from the exact logout position even on a new machine.
    let no_local_save = !PlayerPersistence::has_local_save(&world_dir, &identity);
    if no_local_save {
        println!("🆕 No local save — requesting last session from DHT...");
        multiplayer.fetch_session_record();
    }

    // Prefetch world objects for the Construct area (chunk around origin) and
    // the 3×3 neighbourhood so placed billboards/terminals appear immediately.
    {
        use metaverse_core::world_objects::chunk_coords_for_pos;
        let (ox, oz) = chunk_coords_for_pos(0.0, 0.0);
        multiplayer.fetch_world_objects_for_area(ox, oz, 2);
        println!("🗺️  Requesting world objects for spawn area chunks…");
    }

    // Fetch content for all sections from the server so billboards are populated
    // immediately on first render rather than waiting for gossipsub messages.
    // The channel carries batches of ContentItems from the background thread.
    let (content_inbox_tx, content_inbox_rx) =
        std::sync::mpsc::channel::<Vec<metaverse_core::meshsite::ContentItem>>();
    {
        use metaverse_core::meshsite::{ContentItem, Section};
        let url = server_url.clone();
        let tx  = content_inbox_tx.clone();
        println!("📰 Fetching content from server {}…", url);
        std::thread::spawn(move || {
            let client = match reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build() { Ok(c) => c, Err(_) => return };
            for section in ["forums", "wiki", "marketplace", "post"] {
                let endpoint = format!("{}/api/v1/content?section={}&limit=50", url, section);
                let Ok(resp) = client.get(&endpoint).send() else { continue };
                let Ok(json) = resp.json::<Vec<serde_json::Value>>() else { continue };
                let items: Vec<ContentItem> = json.into_iter().filter_map(|v| {
                    Some(ContentItem {
                        id:         v["id"].as_str()?.to_string(),
                        section:    Section::from_str(v["section"].as_str().unwrap_or("forums"))
                                        .unwrap_or(Section::Forums),
                        title:      v["title"].as_str().unwrap_or("").to_string(),
                        body:       v["body"].as_str().unwrap_or("").to_string(),
                        author:     v["author"].as_str().unwrap_or("").to_string(),
                        signature:  vec![],
                        created_at: v["created_at"].as_u64().unwrap_or(0),
                    })
                }).collect();
                if !items.is_empty() {
                    println!("   📄 {} → {} items", section, items.len());
                    let _ = tx.send(items);
                }
            }
        });
    }

    // Load saved player state (position, rotation, mode) - encrypted with identity
    let player_state = PlayerPersistence::load(&world_dir, &identity);
    println!("🧍 Setting up player...");

    // If saved position is more than 2km from current spawn origin, discard it.
    // This happens when the spawn GPS changes between sessions.
    let spawn_ecef_origin = origin_gps.to_ecef();
    let saved_ecef = player_state.position;
    let dist_from_origin = {
        let dx = saved_ecef.x - spawn_ecef_origin.x;
        let dy = saved_ecef.y - spawn_ecef_origin.y;
        let dz = saved_ecef.z - spawn_ecef_origin.z;
        (dx*dx + dy*dy + dz*dz).sqrt()
    };
    let use_saved = dist_from_origin < 2000.0 && dist_from_origin > 0.001;
    if !use_saved {
        println!("   ⚠️  Saved position {:.0}m from spawn — resetting to spawn", dist_from_origin);
    }

    let initial_position = if use_saved { player_state.position } else { spawn_ecef_origin };
    let initial_gps    = if use_saved { player_state.gps } else { origin_gps };

    // Create player at saved position (or default if no save)
    let mut player = Player::new(&mut physics, initial_gps, player_state.yaw);
    player.position = initial_position;
    player.camera_yaw = player_state.yaw;
    player.camera_pitch = player_state.pitch;
    
    // In Construct mode (always on startup), override position to Construct spawn.
    // Ignore any saved open-world position — Construct has its own floor at local Y=0.
    // Spawn 2.5 m above floor so the capsule (bottom = spawn_y - 0.9) has clear air.
    let spawn_local = metaverse_core::construct::SPAWN_POINT + glam::Vec3::new(0.0, 2.5, 0.0);
    let spawn_ecef = physics.local_to_ecef(spawn_local);
    player.position = spawn_ecef;
    if let Some(body) = physics.bodies.get_mut(player.body_handle) {
        body.set_translation(vector![spawn_local.x, spawn_local.y, spawn_local.z], true);
    }

    // Determine which chunk the player is actually standing in and prioritise it
    // so it is dispatched to a worker thread before any surrounding chunks.
    // Only relevant in OpenWorld mode — in Construct we don't need terrain chunks.
    let player_chunk = ChunkId::from_ecef(&player.position);

    // ── Synchronously generate spawn chunk (OpenWorld only) ───────────────────
    // In Construct mode the flat collision plane is sufficient — no terrain needed.
    // On portal transition this same pattern runs inside the event loop instead.
    if game_mode == GameMode::OpenWorld {
        chunk_streamer.queue_priority(player_chunk);
        println!("   Player chunk: {} — queued with priority", player_chunk);

        let generator = generator_arc.lock().unwrap();
        match generator.generate_chunk(&player_chunk) {
            Ok(octree) => {
                let min_v = player_chunk.min_voxel();
                let max_v = player_chunk.max_voxel();
                let (mut mesh, chunk_center) = extract_chunk_mesh(&octree, &min_v, &max_v);
                if !mesh.vertices.is_empty() {
                    let offset = Vec3::new(
                        (chunk_center.x - origin_voxel.x) as f32,
                        (chunk_center.y - origin_voxel.y) as f32,
                        (chunk_center.z - origin_voxel.z) as f32,
                    );
                    for v in &mut mesh.vertices { v.position += offset; }
                    let collider = metaverse_core::physics::create_collision_from_mesh(
                        &mut physics, &mesh, &origin_voxel, None);
                    chunk_streamer.preload_chunk(player_chunk, octree, Some(collider));
                    println!("✅ Spawn floor ready — terrain is live");
                } else {
                    println!("⚠️  Spawn chunk generated but has no mesh (ocean/void?)");
                }
            }
            Err(e) => eprintln!("⚠️  Could not generate spawn chunk synchronously: {}", e),
        }
    }

    let player_local = physics.ecef_to_local(&player.position);
    println!("✅ Player position at local: ({:.1}, {:.1}, {:.1})", 
        player_local.x, player_local.y, player_local.z);
    
    // Camera setup - first person from player eyes
    let camera_local = player.camera_position_local(&physics);
    let mut camera = Camera::new(camera_local, 1920.0 / 1080.0);
    camera.yaw = player.camera_yaw;
    camera.pitch = player.camera_pitch;
    
    // Model transform bind groups
    let player_model_matrix = Mat4::from_rotation_translation(
        glam::Quat::from_rotation_y(player.camera_yaw),
        player_local
    );
    let (player_model_uniform, player_model_bind_group) = 
        pipeline.create_model_bind_group(&context.device, &player_model_matrix);
    
    let crosshair_matrix = Mat4::IDENTITY;
    let (crosshair_uniform, crosshair_bind_group) = 
        pipeline.create_model_bind_group(&context.device, &crosshair_matrix);
    
    // Remote player bind groups (create one per remote player as needed)
    let mut remote_player_bind_groups: HashMap<libp2p::PeerId, (wgpu::Buffer, wgpu::BindGroup)> = HashMap::new();
    
    // Input state
    let mut input_forward = 0.0f32;
    let mut input_right = 0.0f32;
    let mut input_up = 0.0f32;
    let mut jump_pressed = false;
    let mut dig_pressed = false;
    let mut place_pressed = false;
    let mut chat_pressed = false;
    let mut player_mode = PlayerModeLocal::Walk;
    
    let mut _last_frame = Instant::now();
    let mut frame_count = 0;
    let mut fps_timer = Instant::now();
    let mut last_stats_print = Instant::now();
    let mut last_state_resync = Instant::now();
    let mut last_periodic_save = Instant::now();
    // DHT fallback: query providers for loaded chunks if gossipsub sync hasn't
    // delivered ops after 10s in OpenWorld mode with no peers.
    let mut dht_fallback_at: Option<Instant> = None;
    let mut dht_fallback_done = false;

    // HUD data — updated every physics frame, read by render
    let mut hud_near_terminal: bool = false;
    let mut hud_near_module:   Option<usize> = None;

    // Current WORLDNET address shown on the terminal screen
    let mut terminal_address = metaverse_core::worldnet::WorldnetAddress::Root;
    let mut terminal_active  = false;       // true = keyboard routes to terminal
    let mut terminal_input   = String::new(); // current command being typed
    
    let mut cursor_grabbed = false;
    
    // Track local voxel operations for CRDT merge
    let mut local_voxel_ops: HashMap<VoxelCoord, metaverse_core::messages::SignedOperation> = HashMap::new();

    // Track the world-object chunk we last fetched for, to avoid redundant DHT queries.
    let mut last_world_obj_chunk: Option<(i32, i32)> = None;

    // Loading phase: true until enough spawn-area chunks have meshes and collision built.
    // The event loop renders the loading bar while this is true.
    // In Construct mode we skip terrain loading entirely — floor is ready from frame 1.
    const LOADING_TARGET: usize = 30;
    let mut game_loading = game_mode != GameMode::Construct;
    let mut loading_frames: u32 = 0;  // minimum frames before we allow exit

    println!("\n🌍 Loading spawn area (chunks stream in during first frames)...");
    println!("   Target: {} chunks, spawn chunk must have collider", LOADING_TARGET);
    println!("   Progress will print every second. Window title shows loading status.");

    Box::new(move |event: Event<()>, elwt: &ActiveEventLoop| {
        match event {
            Event::WindowEvent { ref event, .. } => {
                // Route all window events through egui when the signup or compose screen is active
                if let Some(ref mut s) = signup {
                    s.on_event(&window, event);
                }
                if let Some(ref mut c) = compose {
                    c.on_event(&window, event);
                }
                if let Some(ref mut p) = placement {
                    p.on_event(&window, event);
                }
                // HUD always needs events for egui input tracking
                hud.on_event(&window, event);
            }
            _ => {}
        }
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    println!("\n👋 Shutting down...");
                    
                    // Save all voxel operations from the live user_content Arc
                    // (chunk_manager holds a startup clone; saving from the Arc ensures
                    //  remote ops applied during this session are also persisted)
                    println!("💾 Saving world state...");
                    // Save from the live Arc — it holds all ops: local edits, remote ops,
                    // and state-sync ops. save_chunks() deduplicates by signature internally.
                    match user_content.lock().unwrap().save_chunks(&world_dir) {
                        Ok(saved) => {
                            let total: usize = saved.values().sum();
                            println!("   ✅ Saved {} operations across {} chunks", total, saved.len());
                        }
                        Err(e) => {
                            eprintln!("   ⚠️  Failed to save chunks: {}", e);
                        }
                    }
                    
                    // Save player position
                    let player_state = PlayerPersistence::from_state(
                        player.position,
                        player.camera_yaw,
                        player.camera_pitch,
                        if player_mode == PlayerModeLocal::Walk { MovementMode::Walk } else { MovementMode::Fly }
                    );
                    if let Err(e) = player_state.save(&world_dir, &identity) {
                        eprintln!("   ⚠️  Failed to save player position: {}", e);
                    } else {
                        println!("   ✅ Saved player position");
                    }

                    // Also publish session record to DHT so the player can resume
                    // from this exact spot on any machine that has their identity key.
                    {
                        let chunk = ChunkId::from_ecef(&player.position);
                        let movement_mode_byte = if player_mode == PlayerModeLocal::Walk { 0u8 } else { 1u8 };
                        multiplayer.publish_session_record(
                            [player.position.x, player.position.y, player.position.z],
                            [player.camera_yaw, player.camera_pitch],
                            movement_mode_byte,
                            [chunk.x, chunk.y, chunk.z],
                        );
                        println!("   ✅ Published session record to DHT");
                    }
                    
                    println!("   Goodbye!");
                    elwt.exit();
                }
                
                WindowEvent::KeyboardInput { event, .. } => {
                    // Block game input while the signup or compose screen is visible
                    if signup.is_some() { return; }
                    if placement.is_some() {
                        if event.state == ElementState::Pressed {
                            if let PhysicalKey::Code(KeyCode::Escape) = event.physical_key {
                                placement = None;
                                window.set_cursor_visible(false);
                                let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Locked);
                            }
                        }
                        return;
                    }
                    if compose.is_some() {
                        if event.state == ElementState::Pressed {
                            if let PhysicalKey::Code(KeyCode::Escape) = event.physical_key {
                                compose = None;
                            }
                        }
                        return;
                    }
                    // ── WORLDNET terminal input mode ──────────────────────────
                    // When terminal_active, route all keystrokes to the terminal.
                    if terminal_active && event.state == ElementState::Pressed {
                        use metaverse_core::worldnet::{
                            render_page, render_terminal_prompt,
                            process_terminal_command, addr_section, TerminalCmd,
                        };
                        // Helper: re-render page + prompt and upload to screen
                        macro_rules! refresh {
                            () => {{
                                let key_type = identity.load_key_record()
                                    .map(|kr| kr.effective_key_type());
                                let content = multiplayer.get_content(addr_section(&terminal_address));
                                render_page(&terminal_address, key_type, content, &mut worldnet_buf);
                                render_terminal_prompt(&terminal_input, &mut worldnet_buf);
                                terminal_screen.update(&context.queue, &worldnet_buf);
                            }};
                        }
                        match event.physical_key {
                            // Escape / E — close terminal
                            PhysicalKey::Code(KeyCode::Escape)
                            | PhysicalKey::Code(KeyCode::KeyE) => {
                                terminal_active = false;
                                terminal_input.clear();
                                let key_type = identity.load_key_record()
                                    .map(|kr| kr.effective_key_type());
                                let content = multiplayer.get_content(
                                    addr_section(&terminal_address));
                                render_page(&terminal_address, key_type, content, &mut worldnet_buf);
                                terminal_screen.update(&context.queue, &worldnet_buf);
                                let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Locked);
                                window.set_cursor_visible(false);
                                cursor_grabbed = true;
                            }
                            // Enter — execute command
                            PhysicalKey::Code(KeyCode::Enter) => {
                                let cmd = terminal_input.trim().to_string();
                                terminal_input.clear();
                                match process_terminal_command(&cmd, &terminal_address) {
                                    TerminalCmd::Navigate(addr) => {
                                        terminal_address = addr;
                                        refresh!();
                                    }
                                    TerminalCmd::OpenCompose => {
                                        terminal_active = false;
                                        terminal_input.clear();
                                        let author = multiplayer.peer_id().to_string();
                                        compose = Some(ComposeScreen::new(
                                            &context, &window,
                                            metaverse_core::meshsite::Section::Forums,
                                            author,
                                        ));
                                        window.set_cursor_visible(true);
                                        let _ = window.set_cursor_grab(
                                            winit::window::CursorGrabMode::None);
                                    }
                                    TerminalCmd::Close => {
                                        terminal_active = false;
                                        let key_type = identity.load_key_record()
                                            .map(|kr| kr.effective_key_type());
                                        let content = multiplayer.get_content(
                                            addr_section(&terminal_address));
                                        render_page(&terminal_address, key_type, content,
                                                    &mut worldnet_buf);
                                        terminal_screen.update(&context.queue, &worldnet_buf);
                                        let _ = window.set_cursor_grab(
                                            winit::window::CursorGrabMode::Locked);
                                        window.set_cursor_visible(false);
                                        cursor_grabbed = true;
                                    }
                                    TerminalCmd::Refresh => {
                                        refresh!();
                                    }
                                }
                            }
                            // Backspace — delete last char
                            PhysicalKey::Code(KeyCode::Backspace) => {
                                terminal_input.pop();
                                refresh!();
                            }
                            // Any printable character
                            _ => {
                                if let Some(text) = &event.text {
                                    for ch in text.chars() {
                                        if !ch.is_control() && terminal_input.len() < 60 {
                                            terminal_input.push(ch);
                                        }
                                    }
                                    refresh!();
                                }
                            }
                        }
                        return;
                    }
                    if event.state == ElementState::Pressed {
                        if let PhysicalKey::Code(keycode) = event.physical_key {
                            match keycode {
                                KeyCode::Escape => {
                                    window.set_cursor_visible(true);
                                    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                    cursor_grabbed = false;
                                    println!("🖱️  Mouse released");
                                }
                                KeyCode::F12 => {
                                    // TODO: Update screenshot to work with multiple chunk meshes
                                    println!("⚠️  Screenshot temporarily disabled during chunk refactor");
                                    /*
                                    take_screenshot(
                                        &context,
                                        &mut pipeline,
                                        &mut camera,
                                        &player,
                                        &physics,
                                        &mesh_buffer,
                                        &hitbox_buffer,
                                        &player_model_bind_group,
                                    );
                                    */
                                }
                                KeyCode::KeyF => {
                                    player_mode = match player_mode {
                                        PlayerModeLocal::Walk => {
                                            println!("🚀 Fly mode enabled");
                                            PlayerModeLocal::Fly
                                        }
                                        PlayerModeLocal::Fly => {
                                            println!("🚶 Walk mode enabled");
                                            PlayerModeLocal::Walk
                                        }
                                    };
                                }
                                KeyCode::KeyT => {
                                    chat_pressed = true;
                                }
                                KeyCode::KeyW => input_forward = 1.0,
                                KeyCode::KeyS => input_forward = -1.0,
                                KeyCode::KeyA => input_right = -1.0,
                                KeyCode::KeyD => input_right = 1.0,
                                KeyCode::Space => {
                                    if player_mode == PlayerModeLocal::Walk {
                                        jump_pressed = true;
                                    } else {
                                        input_up = 1.0;
                                    }
                                }
                                KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                                    if player_mode == PlayerModeLocal::Fly {
                                        input_up = -1.0;
                                    }
                                }
                                KeyCode::KeyE => {
                                    use metaverse_core::worldnet::{
                                        render_page, render_terminal_prompt, addr_section,
                                    };
                                    if let Some(idx) = hud_near_module {
                                        // Module room: open compose screen for content rooms (2-5)
                                        const MODULE_SECTIONS: [Option<metaverse_core::meshsite::Section>; 6] = [
                                            None, None,
                                            Some(metaverse_core::meshsite::Section::Forums),
                                            Some(metaverse_core::meshsite::Section::Wiki),
                                            Some(metaverse_core::meshsite::Section::Marketplace),
                                            Some(metaverse_core::meshsite::Section::Post),
                                        ];
                                        if let Some(section) = MODULE_SECTIONS[idx].clone() {
                                            let author = multiplayer.peer_id().to_string();
                                            compose = Some(ComposeScreen::new(&context, &window, section, author));
                                            window.set_cursor_visible(true);
                                            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                        }
                                    } else if hud_near_terminal {
                                        // Activate terminal — release cursor, show prompt
                                        terminal_active = true;
                                        terminal_input.clear();
                                        let key_type = identity.load_key_record()
                                            .map(|kr| kr.effective_key_type());
                                        let content = multiplayer.get_content(addr_section(&terminal_address));
                                        render_page(&terminal_address, key_type, content, &mut worldnet_buf);
                                        render_terminal_prompt(&terminal_input, &mut worldnet_buf);
                                        terminal_screen.update(&context.queue, &worldnet_buf);
                                        window.set_cursor_visible(true);
                                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                        cursor_grabbed = false;
                                        println!("🖥️  WORLDNET terminal active — type commands, ESC to exit");
                                    }
                                }
                                // P — open in-game object placement overlay
                                KeyCode::KeyP => {
                                    // Place object 3m ahead of player in look direction
                                    let ploc = physics.ecef_to_local(&player.position);
                                    let yaw = player.camera_yaw;
                                    let pos = [
                                        ploc.x as f32 + yaw.sin() * 3.0,
                                        ploc.y as f32,
                                        ploc.z as f32 + yaw.cos() * 3.0,
                                    ];
                                    // Object faces back toward player
                                    let rot = yaw + std::f32::consts::PI;
                                    let author = multiplayer.peer_id().to_string();
                                    placement = Some(PlacementScreen::new(
                                        &context, &window, pos, rot, author,
                                    ));
                                    window.set_cursor_visible(true);
                                    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                }
                                // Q/E no longer dig/place — use mouse buttons
                                // O — toggle OSM water polygon debug outlines (bright yellow)
                                KeyCode::KeyO => {
                                    show_osm_debug = !show_osm_debug;
                                    println!("🗺️  OSM debug outlines: {}", if show_osm_debug { "ON" } else { "OFF" });
                                    osm_geom_dirty = true; // rebuild debug mesh
                                }
                                _ => {}
                            }
                        }
                    } else if event.state == ElementState::Released {
                        if let PhysicalKey::Code(keycode) = event.physical_key {
                            match keycode {
                                KeyCode::KeyW | KeyCode::KeyS => input_forward = 0.0,
                                KeyCode::KeyA | KeyCode::KeyD => input_right = 0.0,
                                KeyCode::Space | KeyCode::ShiftLeft | KeyCode::ShiftRight => input_up = 0.0,
                                _ => {}
                            }
                        }
                    }
                }
                
                WindowEvent::MouseInput { button, state: ElementState::Pressed, .. } => {
                    // Don't act while signup screen is visible
                    if signup.is_some() { return; }
                    match button {
                        MouseButton::Left => {
                            // Grab cursor on first left-click (enter FPS mode), then dig
                            if !cursor_grabbed {
                                let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                                window.set_cursor_visible(false);
                                cursor_grabbed = true;
                            } else {
                                dig_pressed = true;
                            }
                        }
                        MouseButton::Right => {
                            if cursor_grabbed {
                                place_pressed = true;
                            }
                        }
                        _ => {}
                    }
                }
                
                WindowEvent::Resized(new_size) => {
                    context.resize(new_size);
                    pipeline.resize(&context.device, &context.config);
                    camera.aspect = new_size.width as f32 / new_size.height as f32;
                }
                
                WindowEvent::RedrawRequested => {
                    let dt = PHYSICS_TIMESTEP;

                    // ── Loading phase ──────────────────────────────────────────────
                    if game_loading {
                        // Keep streaming chunks and building meshes each frame
                        chunk_streamer.update(player.position);
                        chunk_streamer.process_queues(80.0);

                        // Build mesh + collider for any chunks that finished loading
                        let new_chunks: Vec<_> = chunk_streamer.newly_loaded_chunks.drain(..).collect();
                        for chunk_id in &new_chunks {
                            if let Some(chunk_data) = chunk_streamer.get_chunk_mut(chunk_id) {
                                let min_v = chunk_data.id.min_voxel();
                                let max_v = chunk_data.id.max_voxel();
                                let (mut mesh, chunk_center) = extract_chunk_mesh(&chunk_data.octree, &min_v, &max_v);
                                let offset = Vec3::new(
                                    (chunk_center.x - origin_voxel.x) as f32,
                                    (chunk_center.y - origin_voxel.y) as f32,
                                    (chunk_center.z - origin_voxel.z) as f32,
                                );
                                if !mesh.vertices.is_empty() {
                                    for v in &mut mesh.vertices { v.position += offset; }
                                    chunk_data.mesh_buffer = Some(MeshBuffer::from_mesh(&context.device, &mesh));
                                    let collider = metaverse_core::physics::create_collision_from_mesh(
                                        &mut physics, &mesh, &origin_voxel, None);
                                    chunk_data.collider = Some(collider);
                                }
                                // Water surface mesh (flat quads on top of WATER voxels)
                                let mut water_mesh = extract_water_surface_mesh(&chunk_data.octree, &min_v, &max_v);
                                if !water_mesh.vertices.is_empty() {
                                    for v in &mut water_mesh.vertices { v.position += offset; }
                                    chunk_data.water_mesh_buffer = Some(MeshBuffer::from_mesh(&context.device, &water_mesh));
                                }
                                chunk_data.dirty = false;
                            }
                        }

                        // OSM inference for newly loaded chunks
                        {
                            use metaverse_core::world_inference::{ChunkContext, infer_chunk_objects,
                                                                  infer_road_segments, to_placed_object};
                            for &chunk_id in &new_chunks {
                                let (lat_min, lat_max, lon_min, lon_max) = chunk_id.gps_bounds();
                                let osm = metaverse_core::osm::fetch_osm_for_chunk(
                                    lat_min, lat_max, lon_min, lon_max, &osm_cache_dir);
                                let ctx = if osm.is_empty() {
                                    ChunkContext::from_chunk(chunk_id)
                                } else {
                                    ChunkContext::from_chunk(chunk_id).with_osm(osm)
                                };
                                // Buildings: Y = srtm - origin_alt (same formula as terrain generator).
                                // X/Z from GPS(lat,lon,0).to_ecef() → ecef_to_local (horizontal only).
                                let inferred = infer_chunk_objects(&ctx);
                                if !inferred.is_empty() {
                                    for obj in &inferred {
                                        let srtm = osm_elev_pipeline.query(
                                            &metaverse_core::coordinates::GPS::new(obj.lat, obj.lon, 0.0))
                                            .map(|e| e.meters)
                                            .unwrap_or(origin_gps.alt);
                                        // X/Z: sea-level ECEF horizontal (matches terrain column coords).
                                        // Y:   geographic altitude diff from origin (matches terrain voxel Y).
                                        let horiz = physics.ecef_to_local(
                                            &metaverse_core::coordinates::GPS::new(obj.lat, obj.lon, 0.0).to_ecef());
                                        let world_pos = [horiz.x, (srtm - origin_gps.alt) as f32, horiz.z];
                                        multiplayer.register_inferred_object(to_placed_object(obj, world_pos));
                                    }
                                    osm_geom_dirty = true;
                                }
                                let segs = infer_road_segments(&ctx);
                                if !segs.is_empty() {
                                    for seg in &segs {
                                        let sa = osm_elev_pipeline.query(
                                            &metaverse_core::coordinates::GPS::new(seg.a_lat, seg.a_lon, 0.0))
                                            .map(|e| e.meters)
                                            .unwrap_or(origin_gps.alt);
                                        let sb = osm_elev_pipeline.query(
                                            &metaverse_core::coordinates::GPS::new(seg.b_lat, seg.b_lon, 0.0))
                                            .map(|e| e.meters)
                                            .unwrap_or(origin_gps.alt);
                                        // X/Z from sea-level ECEF; Y = altitude diff from origin.
                                        let la = physics.ecef_to_local(
                                            &metaverse_core::coordinates::GPS::new(seg.a_lat, seg.a_lon, 0.0).to_ecef());
                                        let lb = physics.ecef_to_local(
                                            &metaverse_core::coordinates::GPS::new(seg.b_lat, seg.b_lon, 0.0).to_ecef());
                                        let pa = Vec3::new(la.x, (sa - origin_gps.alt) as f32 + 0.05, la.z);
                                        let pb = Vec3::new(lb.x, (sb - origin_gps.alt) as f32 + 0.05, lb.z);
                                        road_segments.push((pa, pb, seg.width_m, seg.road_type.clone()));
                                        if let Some(ref name) = seg.name {
                                            let mid = Vec3::new((pa.x+pb.x)*0.5, (pa.y+pb.y)*0.5+3.0, (pa.z+pb.z)*0.5);
                                            if !road_labels.iter().any(|(_, n)| n == name) {
                                                road_labels.push((mid, name.clone()));
                                            }
                                        }
                                    }
                                    osm_geom_dirty = true;
                                }
                                // Water polygons — flat at minimum sampled vertex elevation.
                                // Use min elevation (not centroid) since river polygon centroids
                                // often fall on land far from the chunk, giving wrong Y.
                                if let Some(ref osm_data) = ctx.osm {
                                    if !osm_data.water.is_empty() {
                                        for w in &osm_data.water {
                                            if w.polygon.len() < 3 { continue; }
                                            let key = ((w.polygon[0].lat * 1e6) as i64,
                                                       (w.polygon[0].lon * 1e6) as i64);
                                            if !water_poly_seen.insert(key) { continue; }
                                            // Per-vertex elevation: each vertex gets its own Y from SRTM.
                                            // The river flows downhill — 15m at upstream end, 5m at CBD,
                                            // 0m at Moreton Bay over 345km. Forcing all vertices to the
                                            // same Y creates a second floating plane where the polygon
                                            // is above or below the terrain at the wrong elevation.
                                            // With per-vertex Y the water surface drapes along the slope.
                                            let verts: Vec<Vec3> = w.polygon.iter().map(|gps| {
                                                let h = physics.ecef_to_local(
                                                    &metaverse_core::coordinates::GPS::new(gps.lat, gps.lon, 0.0).to_ecef());
                                                let elev = osm_elev_pipeline.query(
                                                    &metaverse_core::coordinates::GPS::new(gps.lat, gps.lon, 0.0))
                                                    .map(|e| e.meters)
                                                    .unwrap_or(2.0);
                                                let vy = (elev - origin_gps.alt) as f32 + 0.05;
                                                Vec3::new(h.x, vy, h.z)
                                            }).collect();
                                            water_polygons.push(verts);
                                        }
                                        osm_geom_dirty = true;
                                    }
                                }
                            }
                        }

                        loading_frames += 1;

                        let loaded = chunk_streamer.stats.chunks_loaded;
                        let generating = chunk_streamer.stats.chunks_loading;
                        let queued    = chunk_streamer.stats.chunks_queued;

                        // Progress feedback every second (~60 frames) so the user
                        // can see the black loading window is actually doing work.
                        if loading_frames % 60 == 1 {
                            let player_status = if chunk_streamer
                                .get_chunk(&player_chunk)
                                .map(|c| c.collider.is_some())
                                .unwrap_or(false) { "ready" } else { "waiting" };
                            println!("⏳ Loading chunks: {}/{} | generating: {} | queued: {} | player chunk: {}",
                                loaded, LOADING_TARGET, generating, queued, player_status);
                            let _ = window.set_title(&format!(
                                "Metaverse — Loading {}/{} chunks (player: {})",
                                loaded, LOADING_TARGET, player_status));
                        }

                        // Transition to game only when:
                        //  1. Minimum frames elapsed
                        //  2. The chunk the player is ACTUALLY standing in has a collider
                        //     (prevents falling through terrain on first physics step)
                        //  3. Enough surrounding chunks are also ready (or queue drained)
                        let player_chunk_ready = chunk_streamer
                            .get_chunk(&player_chunk)
                            .map(|c| c.collider.is_some())
                            .unwrap_or(false);
                        let enough_chunks = loaded >= LOADING_TARGET;
                        let queue_drained = chunk_streamer.stats.chunks_loading == 0
                            && chunk_streamer.stats.chunks_queued == 0
                            && loaded > 0;

                        if loading_frames >= 20 && player_chunk_ready && (enough_chunks || queue_drained) {
                            println!("✅ Spawn area loaded ({} chunks), player chunk ready — starting game", loaded);
                            let _ = window.set_title("Metaverse");

                            // Request historical state from peers now that we have chunks
                            if multiplayer.peer_count() > 0 {
                                let ids = chunk_streamer.loaded_chunk_ids();
                                let _ = multiplayer.request_chunk_state(ids);
                            } else {
                                // No peers connected — schedule DHT fallback in 10s
                                dht_fallback_at = Some(Instant::now());
                                dht_fallback_done = false;
                            }
                            println!("🎮 Game started!");
                            game_loading = false;
                        } else if loading_frames >= 600 {
                            // Safety timeout (~10s at 60fps) — start anyway if stuck
                            println!("⚠️  Loading timeout — starting with {} chunks (generating: {}, player chunk: {})",
                                loaded, generating, player_chunk_ready);
                            let _ = window.set_title("Metaverse");
                            if multiplayer.peer_count() > 0 {
                                let ids = chunk_streamer.loaded_chunk_ids();
                                let _ = multiplayer.request_chunk_state(ids);
                            } else {
                                dht_fallback_at = Some(Instant::now());
                                dht_fallback_done = false;
                            }
                            println!("🎮 Game started (timeout)!");
                            game_loading = false;
                        }
                        return;
                    }
                    // ── End loading phase ─────────────────────────────────────────

                    // Update multiplayer system (polls network, interpolates remote players)
                    multiplayer.update(dt);

                    // Drain any content items that arrived from the server HTTP fetch thread
                    while let Ok(items) = content_inbox_rx.try_recv() {
                        multiplayer.inject_content(items);
                        billboard_frame_counter = 0; // trigger billboard rebuild
                    }
                    
                    // Handle chat
                    if chat_pressed {
                        let _ = multiplayer.send_chat("Hello from P2P!".to_string());
                        println!("💬 Sent chat message");
                        chat_pressed = false;
                    }
                    
                    // Handle digging
                    if dig_pressed && TERRAIN_IS_EDITABLE {
                        // Find which chunk the raycast will hit (we need to check all loaded chunks)
                        let camera_local = player.camera_position_local(&physics);
                        let camera_ecef = physics.local_to_ecef(camera_local);
                        let camera_dir = player.camera_forward();
                        
                        // Try raycasting in each loaded chunk to find hit
                        let mut hit_coord = None;
                        let mut hit_chunk_id = None;
                        for chunk_data in chunk_streamer.loaded_chunks_mut() {
                            if let Some(hit) = metaverse_core::voxel::raycast_voxels(
                                &chunk_data.octree,
                                &camera_ecef,
                                camera_dir,
                                10.0
                            ) {
                                hit_coord = Some(hit.voxel);
                                hit_chunk_id = Some(chunk_data.id);
                                // Dig the voxel
                                chunk_data.octree.set_voxel(hit.voxel, MaterialId::AIR);
                                chunk_data.dirty = true;
                                break;
                            }
                        }
                        if let Some(id) = hit_chunk_id {
                            chunk_streamer.touch_chunk(&id);
                        }
                        
                        if let Some(dug) = hit_coord {
                            println!("⛏️  Dug voxel at {:?}", dug);
                            
                            // Broadcast voxel operation
                            if let Ok(op) = multiplayer.broadcast_voxel_operation(dug, Material::Air) {
                                // Save to user content layer (persistence)
                                user_content.lock().unwrap().add_local_operation(op.clone());
                                
                                // Track for CRDT merges
                                chunk_manager.add_operation(op.clone());
                                local_voxel_ops.insert(dug, op);

                                // Advertise this chunk to DHT so offline peers can find us later
                                let edited_chunk = ChunkId::from_voxel(&dug);
                                multiplayer.advertise_chunks(&[edited_chunk]);
                            }
                        }
                        dig_pressed = false;
                    }
                    
                    // Handle placing
                    if place_pressed && TERRAIN_IS_EDITABLE {
                        // Find which chunk the raycast will hit
                        let camera_local = player.camera_position_local(&physics);
                        let camera_ecef = physics.local_to_ecef(camera_local);
                        let camera_dir = player.camera_forward();
                        
                        // Try raycasting in each loaded chunk to find hit
                        let mut place_info: Option<(VoxelCoord, ChunkId)> = None;
                        for chunk_data in chunk_streamer.loaded_chunks() {
                            if let Some(hit) = metaverse_core::voxel::raycast_voxels(
                                &chunk_data.octree,
                                &camera_ecef,
                                camera_dir,
                                10.0
                            ) {
                                // Place on the face that was hit (adjacent to hit voxel)
                                let place_voxel = VoxelCoord::new(
                                    hit.voxel.x + hit.face_normal.0,
                                    hit.voxel.y + hit.face_normal.1,
                                    hit.voxel.z + hit.face_normal.2,
                                );
                                
                                // Check player collision before placing
                                let place_local = physics.ecef_to_local(&place_voxel.to_ecef());
                                let player_local = physics.ecef_to_local(&player.position);
                                let capsule_radius = 0.4;
                                let capsule_height = 1.8;
                                
                                // Check if voxel would overlap with player capsule
                                // Player position is at feet, capsule extends up
                                let dx = (place_local.x - player_local.x).abs();
                                let dy = place_local.y - player_local.y;  // Relative Y (positive = above player)
                                let dz = (place_local.z - player_local.z).abs();
                                
                                // Horizontal distance check (XZ plane)
                                let horizontal_dist = (dx * dx + dz * dz).sqrt();
                                
                                // Only block placement if voxel is:
                                // - Within capsule radius horizontally AND
                                // - Between player's feet and head (0 to capsule_height)
                                let blocks_player = horizontal_dist < capsule_radius && dy >= 0.0 && dy <= capsule_height;
                                
                                if !blocks_player {
                                    // Voxel doesn't intersect player - safe to place
                                    let place_chunk_id = ChunkId::from_voxel(&place_voxel);
                                    place_info = Some((place_voxel, place_chunk_id));
                                } else {
                                    println!("⚠️  Can't place block inside player!");
                                }
                                break;
                            }
                        }
                        
                        // Now apply the placement (after iteration is done)
                        if let Some((place_voxel, place_chunk_id)) = place_info {
                            if let Some(place_chunk) = chunk_streamer.get_chunk_mut(&place_chunk_id) {
                                place_chunk.octree.set_voxel(place_voxel, MaterialId::STONE);
                                place_chunk.dirty = true;
                                chunk_streamer.touch_chunk(&place_chunk_id);
                                
                                println!("🧱 Placed voxel at {:?}", place_voxel);
                                
                                // Broadcast voxel operation and save to user content
                                if let Ok(op) = multiplayer.broadcast_voxel_operation(place_voxel, Material::Stone) {
                                    // Save to user content layer (persistence)
                                    user_content.lock().unwrap().add_local_operation(op.clone());
                                    
                                    // Track for CRDT merges
                                    chunk_manager.add_operation(op.clone());
                                    local_voxel_ops.insert(place_voxel, op);

                                    // Advertise this chunk to DHT
                                    multiplayer.advertise_chunks(&[place_chunk_id]);
                                }
                            }
                        }
                        place_pressed = false;
                    }
                    
                    // Apply any session record that arrived from DHT (new machine / first login)
                    if let Some(session) = multiplayer.take_pending_session_record() {
                        let dht_ecef = ECEF {
                            x: session.position[0],
                            y: session.position[1],
                            z: session.position[2],
                        };
                        // Only restore if within 2km of spawn — same sanity check as local save
                        let dx = dht_ecef.x - spawn_ecef_origin.x;
                        let dy = dht_ecef.y - spawn_ecef_origin.y;
                        let dz = dht_ecef.z - spawn_ecef_origin.z;
                        let dist = (dx*dx + dy*dy + dz*dz).sqrt();
                        if dist < 2000.0 {
                            player.position = dht_ecef;
                            player.camera_yaw   = session.rotation[0];
                            player.camera_pitch = session.rotation[1];
                            println!("📍 Restored position from DHT session record ({:.0}m from spawn)", dist);
                        } else {
                            println!("📍 DHT session record too far ({:.0}m) — keeping spawn", dist);
                        }
                    }

                    // Process any received voxel operations
                    let pending_ops = multiplayer.take_pending_operations();
                    if !pending_ops.is_empty() {
                        println!("📦 Processing {} received voxel operations", pending_ops.len());
                        for op in pending_ops {
                            // Apply to the appropriate chunk
                            if let (Some(coord), Some(material)) = (op.coord(), op.material()) {
                                let chunk_id = ChunkId::from_voxel(&coord);
                                if let Some(chunk_data) = chunk_streamer.get_chunk_mut(&chunk_id) {
                                    let material_id = material.to_material_id();
                                    chunk_data.octree.set_voxel(coord, material_id);
                                    chunk_data.dirty = true;

                                    // Save to BOTH user_content (for ChunkStreamer persistence) AND chunk_manager (for CRDT)
                                    user_content.lock().unwrap().add_local_operation(op.clone());
                                    chunk_manager.add_operation(op.clone());

                                    println!("✅ Applied remote voxel operation at {:?}", coord);
                                } else {
                                    // Operation for unloaded chunk - still save it for when chunk loads
                                    user_content.lock().unwrap().add_local_operation(op.clone());
                                    chunk_manager.add_operation(op.clone());
                                    println!("⚠️  Remote operation for unloaded chunk {} - saved for later", chunk_id);
                                }
                            }
                        }
                    }
                    
                    // Process any received state synchronization operations
                    let state_ops = multiplayer.take_pending_state_operations();
                    if !state_ops.is_empty() {
                        println!("📥 Merging {} historical operations from peers", state_ops.len());
                        
                        // Apply to chunk_manager for CRDT
                        let applied = chunk_manager.merge_received_operations(state_ops.clone());
                        
                        // Also save to user_content for persistence
                        for op in &state_ops {
                            user_content.lock().unwrap().add_local_operation(op.clone());

                            // Apply to loaded chunks if they're in memory
                            if let (Some(coord), Some(material)) = (op.coord(), op.material()) {
                                let chunk_id = ChunkId::from_voxel(&coord);
                                if let Some(chunk_data) = chunk_streamer.get_chunk_mut(&chunk_id) {
                                    let material_id = material.to_material_id();
                                    chunk_data.octree.set_voxel(coord, material_id);
                                    chunk_data.dirty = true;
                                }
                            }
                        }
                        
                        println!("   ✅ Applied {} operations (after deduplication)", applied);
                    }
                    
                    // Check for newly discovered peers and perform full bidirectional state sync
                    if multiplayer.has_new_peers() {
                        let new_peers = multiplayer.get_new_peers();
                        println!("🆕 Detected {} new peers, syncing state...", new_peers.len());
                        let loaded_chunk_ids = chunk_streamer.loaded_chunk_ids();

                        // Send our chunk manifest so peer knows what we have and when.
                        // Each side sends manifests; each side sends chunks where theirs is newer.
                        // This prevents mutual overwrite and the terrain cliff feedback loop.
                        let manifest = chunk_streamer.chunk_manifest();
                        println!("📋 Broadcasting chunk manifest ({} entries)", manifest.len());
                        if let Err(e) = multiplayer.broadcast_chunk_manifest(manifest) {
                            eprintln!("   ⚠️  Failed to broadcast manifest: {}", e);
                        }

                        // Request their op state (pull)
                        if let Err(e) = multiplayer.request_chunk_state(loaded_chunk_ids.clone()) {
                            eprintln!("   ⚠️  Failed to request chunk state: {}", e);
                        }

                        // Push our ops proactively so they don't have to wait for request round-trip
                        let our_ops: std::collections::HashMap<_, _> = {
                            let cl = VectorClock::new(); // empty clock = send all
                            chunk_manager.filter_operations_for_chunks(&loaded_chunk_ids, &cl)
                        };
                        if !our_ops.is_empty() {
                            let count: usize = our_ops.values().map(|v| v.len()).sum();
                            println!("📤 Pushing {} ops to new peer(s)", count);
                            if let Err(e) = multiplayer.send_chunk_state_response(our_ops) {
                                eprintln!("   ⚠️  Failed to push state: {}", e);
                            }
                        }
                        last_state_resync = Instant::now();

                        // Ensure AOI subscriptions are current when a peer arrives.
                        // Without this, if we loaded chunks while alone (so update_subscribed_chunks
                        // never ran) we'd miss per-chunk gossipsub messages from this new peer.
                        if game_mode == GameMode::OpenWorld {
                            let loaded_set: std::collections::HashSet<metaverse_core::chunk::ChunkId> =
                                chunk_streamer.loaded_chunk_ids().iter().copied().collect();
                            let _ = multiplayer.update_subscribed_chunks(&loaded_set);
                        }
                    }
                    if multiplayer.peer_count() > 0 && last_state_resync.elapsed().as_secs() >= 60 {
                        println!("🔁 Periodic state resync with peers...");
                        let loaded_chunk_ids = chunk_streamer.loaded_chunk_ids();
                        if let Err(e) = multiplayer.request_chunk_state(loaded_chunk_ids) {
                            eprintln!("   ⚠️  Periodic resync request failed: {}", e);
                        }
                        last_state_resync = Instant::now();
                    }

                    // Periodic save every 30s — guard against crash data loss
                    if last_periodic_save.elapsed().as_secs() >= 30 {
                        match user_content.lock().unwrap().save_chunks(&world_dir) {
                            Ok(saved) if !saved.is_empty() => {
                                let total: usize = saved.values().sum();
                                println!("💾 [AutoSave] {} ops across {} chunks", total, saved.len());
                                // Re-advertise chunks we just saved
                                multiplayer.advertise_chunks(&saved.keys().cloned().collect::<Vec<_>>());
                            }
                            Ok(_) => {} // nothing to save
                            Err(e) => eprintln!("⚠️  [AutoSave] Failed: {}", e),
                        }
                        last_periodic_save = Instant::now();
                    }

                    // DHT fallback: if no peers connected after 10s, query DHT providers
                    // for all loaded chunks. When providers respond, dial them — once
                    // connected the regular gossipsub sync delivers their ops.
                    if !dht_fallback_done && game_mode == GameMode::OpenWorld {
                        if multiplayer.peer_count() == 0 {
                            if let Some(fallback_start) = dht_fallback_at {
                                if fallback_start.elapsed().as_secs() >= 10 {
                                    let ids = chunk_streamer.loaded_chunk_ids();
                                    multiplayer.query_missing_chunks(&ids);
                                    dht_fallback_done = true;
                                }
                            }
                        } else {
                            // Peers connected — gossipsub sync is handling it
                            dht_fallback_done = true;
                        }
                    }

                    // Process provider results from DHT — dial any unknown providers
                    let provider_results = multiplayer.take_pending_chunk_providers();
                    for (key, providers) in provider_results {
                        // Convert DHT key back to chunk ID for logging
                        let key_str = String::from_utf8_lossy(&key);
                        println!("🗄️  [DHT] Got {} provider(s) for {}", providers.len(), key_str);
                        for provider in providers {
                            if multiplayer.is_connected_peer(&provider) {
                                // Already connected — request their ops directly
                                let ids = chunk_streamer.loaded_chunk_ids();
                                let _ = multiplayer.request_chunk_state(ids);
                            } else {
                                // Not connected — try dialing; once connected the
                                // peer-connect sync path will request their ops
                                println!("   → Dialing provider {}", provider);
                                multiplayer.connect_to_provider(provider);
                            }
                        }
                    }

                    // Handle state requests from peers
                    let state_requests = multiplayer.take_pending_state_requests();
                    for (peer_id, request) in state_requests {
                        println!("📨 Processing state request from {} for {} chunks",
                            peer_id, request.chunk_ids.len());
                        
                        let filtered_ops = chunk_manager.filter_operations_for_chunks(
                            &request.chunk_ids,
                            &request.requester_clock
                        );
                        
                        if !filtered_ops.is_empty() {
                            println!("   → Sending {} operations across {} chunks",
                                filtered_ops.values().map(|v| v.len()).sum::<usize>(),
                                filtered_ops.len()
                            );
                            if let Err(e) = multiplayer.send_chunk_state_response(filtered_ops) {
                                eprintln!("   ⚠️  Failed to send state response: {}", e);
                            }
                        } else {
                            println!("   → No new operations to send");
                        }
                    }

                    // Process received chunk manifests — send chunks where we are newer
                    let manifests = multiplayer.take_pending_chunk_manifests();
                    for peer_manifest in manifests {
                        let peer_map: std::collections::HashMap<ChunkId, u64> = peer_manifest.into_iter().collect();
                        let mut sent = 0;
                        for chunk_id in chunk_streamer.loaded_chunk_ids() {
                            if let Some(chunk) = chunk_streamer.get_chunk(&chunk_id) {
                                let peer_ts = peer_map.get(&chunk_id).copied().unwrap_or(0);
                                if chunk.last_modified > peer_ts {
                                    // We have a newer version — send it
                                    match chunk.octree.to_bytes() {
                                        Ok(bytes) => {
                                            if let Err(e) = multiplayer.broadcast_chunk_terrain(chunk_id, bytes, chunk.last_modified) {
                                                eprintln!("   ⚠️  Failed to send terrain for {:?}: {}", chunk_id, e);
                                            } else {
                                                sent += 1;
                                            }
                                        }
                                        Err(e) => eprintln!("   ⚠️  Failed to serialize chunk {:?}: {}", chunk_id, e),
                                    }
                                }
                            }
                        }
                        if sent > 0 {
                            println!("📦 [TERRAIN SYNC] Sent {} chunks newer than peer", sent);
                        } else {
                            println!("📋 [TERRAIN SYNC] Peer has same or newer terrain, no chunks sent");
                        }
                    }

                    // Apply received chunk terrain data — only if received timestamp is newer than ours
                    let terrain_updates = multiplayer.take_pending_chunk_terrain();
                    if !terrain_updates.is_empty() {
                        println!("🌍 [TERRAIN SYNC] Processing {} chunk terrain updates from peers", terrain_updates.len());
                        for (chunk_id, octree_bytes, last_modified) in terrain_updates {
                            match metaverse_core::voxel::Octree::from_bytes(&octree_bytes) {
                                Ok(octree) => {
                                    if chunk_streamer.replace_chunk_octree(&chunk_id, octree, last_modified) {
                                        println!("   ✅ Applied newer terrain for chunk {:?} (t={})", chunk_id, last_modified);
                                    } else {
                                        println!("   ⏭️  Chunk {:?} rejected (our version same/newer, or not loaded)", chunk_id);
                                    }
                                }
                                Err(e) => eprintln!("   ⚠️  Failed to deserialize terrain for {:?}: {}", chunk_id, e),
                            }
                        }
                    }
                    
                    // Update player movement
                    let move_input = Vec3::new(input_right, input_up, input_forward);
                    
                    if player_mode == PlayerModeLocal::Walk {
                        physics.query_pipeline.update(&physics.colliders);
                        player.update_ground_detection(&physics);
                        player.apply_movement(&mut physics, move_input, jump_pressed, dt);
                        player.sync_from_physics(&physics);
                        physics.step(Vec3::ZERO);
                    } else {
                        const FLY_SPEED: f32 = 10.0;
                        let forward = player.camera_forward();
                        let right = player.camera_right();
                        let up = Vec3::Y;
                        let fly_direction = forward * move_input.z + right * move_input.x + up * move_input.y;
                        
                        if fly_direction.length_squared() > 0.001 {
                            let movement = fly_direction.normalize() * FLY_SPEED * dt;
                            let current_local = physics.ecef_to_local(&player.position);
                            let new_local = current_local + movement;
                            player.position = physics.local_to_ecef(new_local);
                        }
                    }
                    
                    // Broadcast player state AFTER movement update (20 Hz with internal timer)
                    let movement_mode = match player_mode {
                        PlayerModeLocal::Walk => MovementMode::Walk,
                        PlayerModeLocal::Fly => MovementMode::Fly,
                    };
                    
                    let player_local_pos = physics.ecef_to_local(&player.position);
                    let velocity = [player.velocity.x, player.velocity.y, player.velocity.z];
                    
                    let _ = multiplayer.broadcast_player_state(
                        player.position,
                        velocity,
                        player.camera_yaw,
                        player.camera_pitch,
                        movement_mode,
                    );
                    
                    // ── Construct proximity checks ────────────────────────────
                    // Check if player is near an interactive construct object.
                    let ploc = player_local_pos;
                    let ploc3 = Vec3::new(ploc.x, ploc.y, ploc.z);
                    let dist_portal   = (WORLD_PORTAL_POS   - ploc3).length();
                    let dist_terminal = (SIGNUP_TERMINAL_POS - ploc3).length();
                    let near_signup = dist_terminal < INTERACT_RADIUS;
                    let near_portal = dist_portal   < INTERACT_RADIUS;

                    // Detect nearest module screen wall within interact radius
                    // Player must be INSIDE the room (near the back wall) to interact.
                    hud_near_module = MODULES.iter().enumerate()
                        .map(|(i, m)| (i, (m.screen_wall_pos() - ploc3).length()))
                        .filter(|(_, d)| *d < MODULE_DOOR_RADIUS)
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                        .map(|(i, _)| i);

                    hud_near_terminal = near_signup;

                    // Update terminal WORLDNET screen when player is nearby
                    // (every 30 frames to avoid re-rendering every frame)
                    if matches!(game_mode, GameMode::Construct)
                        && frame_count % 30 == 0
                        && dist_terminal < 8.0
                    {
                        use metaverse_core::worldnet::{WorldnetAddress, render_page};
                        let key_type = identity.load_key_record()
                            .map(|kr| kr.effective_key_type());
                        let addr = WorldnetAddress::Root;
                        let content = multiplayer.get_content("forums");
                        render_page(&addr, key_type, content, &mut worldnet_buf);
                        terminal_screen.update(&context.queue, &worldnet_buf);
                    }

                    // Fetch world objects when the player moves into a new chunk
                    {
                        use metaverse_core::world_objects::chunk_coords_for_pos;
                        let current_chunk = chunk_coords_for_pos(ploc3.x, ploc3.z);
                        if last_world_obj_chunk != Some(current_chunk) {
                            last_world_obj_chunk = Some(current_chunk);
                            let (cx, cz) = current_chunk;
                            multiplayer.fetch_world_objects_for_area(cx, cz, 2);
                        }
                    }

                    // Auto-trigger signup overlay if player walks to terminal
                    // and no identity exists yet.
                    if near_signup && signup.is_none() && !Identity::key_file_exists() {
                        println!("🖥️  [Construct] Player at signup terminal");
                        signup = Some(SignupScreen::new(&context, &window));
                    }

                    // World portal: walk through to enter the open world.
                    if near_portal && game_mode == GameMode::Construct {
                        println!("🌐 Walking through world portal — entering Open World...");
                        game_mode = GameMode::OpenWorld;

                        // 1. Compute open world spawn (GPS origin + 3 m above terrain).
                        let world_local_raw = physics.ecef_to_local(&spawn_ecef_origin);
                        let world_spawn_local = Vec3::new(
                            world_local_raw.x,
                            world_local_raw.y + 3.0,
                            world_local_raw.z,
                        );
                        let world_spawn_ecef = physics.local_to_ecef(world_spawn_local);
                        player.position = world_spawn_ecef;

                        // 2. Teleport physics body, zero velocity.
                        if let Some(body) = physics.bodies.get_mut(player.body_handle) {
                            body.set_translation(
                                vector![world_spawn_local.x, world_spawn_local.y, world_spawn_local.z],
                                true,
                            );
                            body.set_linvel(vector![0.0, 0.0, 0.0], true);
                        }

                        // 3. Synchronously generate spawn chunk so player lands on ground.
                        let spawn_chunk = ChunkId::from_ecef(&player.position);
                        chunk_streamer.queue_priority(spawn_chunk);
                        {
                            let generator = generator_arc.lock().unwrap();
                            if let Ok(octree) = generator.generate_chunk(&spawn_chunk) {
                                let min_v = spawn_chunk.min_voxel();
                                let max_v = spawn_chunk.max_voxel();
                                let (mut mesh, chunk_center) = extract_chunk_mesh(&octree, &min_v, &max_v);
                                if !mesh.vertices.is_empty() {
                                    let offset = Vec3::new(
                                        (chunk_center.x - origin_voxel.x) as f32,
                                        (chunk_center.y - origin_voxel.y) as f32,
                                        (chunk_center.z - origin_voxel.z) as f32,
                                    );
                                    for v in &mut mesh.vertices { v.position += offset; }
                                    let collider = metaverse_core::physics::create_collision_from_mesh(
                                        &mut physics, &mesh, &origin_voxel, None);
                                    chunk_streamer.preload_chunk(spawn_chunk, octree, Some(collider));
                                    println!("✅ World spawn chunk ready — ground is live");
                                } else {
                                    println!("⚠️  Spawn chunk is empty (ocean/void?) — player may fall");
                                }
                            }
                        }

                        // 4. Re-enter loading phase so terrain streams in before gameplay.
                        game_loading = true;
                        loading_frames = 0;
                        // Reset DHT fallback so it re-evaluates after terrain loads
                        dht_fallback_at = None;
                        dht_fallback_done = false;

                        // 5. Kick off surrounding chunk streaming.
                        chunk_streamer.update(player.position);

                        println!("🌍 Open World — local ({:.1}, {:.1}, {:.1})",
                            world_spawn_local.x, world_spawn_local.y, world_spawn_local.z);
                    }

                    jump_pressed = false;
                    
                    // Terrain streaming only runs in Open World mode.
                    if game_mode == GameMode::OpenWorld {
                        const FRAME_BUDGET_MS: f64 = 16.0;
                        chunk_streamer.update(player.position);
                        chunk_streamer.process_queues(FRAME_BUDGET_MS);

                    // Broadcast newly loaded chunk manifests to connected peers.
                    // This lets peers replace their independently-generated terrain with ours
                    // if they haven't loaded this chunk yet (or ours is newer due to user edits).
                    if !chunk_streamer.newly_loaded_chunks.is_empty() {
                        // OSM inference for newly loaded chunks (game phase)
                        {
                            use metaverse_core::world_inference::{ChunkContext, infer_chunk_objects,
                                                                  infer_road_segments, to_placed_object};
                            let new_ids: Vec<_> = chunk_streamer.newly_loaded_chunks.iter().copied().collect();
                            for chunk_id in new_ids {
                                let (lat_min, lat_max, lon_min, lon_max) = chunk_id.gps_bounds();
                                let osm = metaverse_core::osm::fetch_osm_for_chunk(
                                    lat_min, lat_max, lon_min, lon_max, &osm_cache_dir);
                                let ctx = if osm.is_empty() {
                                    ChunkContext::from_chunk(chunk_id)
                                } else {
                                    ChunkContext::from_chunk(chunk_id).with_osm(osm)
                                };
                                let inferred = infer_chunk_objects(&ctx);
                                if !inferred.is_empty() {
                                    for obj in &inferred {
                                        let srtm = osm_elev_pipeline.query(
                                            &metaverse_core::coordinates::GPS::new(obj.lat, obj.lon, 0.0))
                                            .map(|e| e.meters)
                                            .unwrap_or(origin_gps.alt);
                                        // X/Z: sea-level ECEF horizontal (matches terrain column coords).
                                        // Y:   geographic altitude diff from origin (matches terrain voxel Y).
                                        let horiz = physics.ecef_to_local(
                                            &metaverse_core::coordinates::GPS::new(obj.lat, obj.lon, 0.0).to_ecef());
                                        let world_pos = [horiz.x, (srtm - origin_gps.alt) as f32, horiz.z];
                                        multiplayer.register_inferred_object(to_placed_object(obj, world_pos));
                                    }
                                    osm_geom_dirty = true;
                                }
                                let segs = infer_road_segments(&ctx);
                                if !segs.is_empty() {
                                    for seg in &segs {
                                        let sa = osm_elev_pipeline.query(
                                            &metaverse_core::coordinates::GPS::new(seg.a_lat, seg.a_lon, 0.0))
                                            .map(|e| e.meters)
                                            .unwrap_or(origin_gps.alt);
                                        let sb = osm_elev_pipeline.query(
                                            &metaverse_core::coordinates::GPS::new(seg.b_lat, seg.b_lon, 0.0))
                                            .map(|e| e.meters)
                                            .unwrap_or(origin_gps.alt);
                                        // X/Z from sea-level ECEF; Y = altitude diff from origin.
                                        let la = physics.ecef_to_local(
                                            &metaverse_core::coordinates::GPS::new(seg.a_lat, seg.a_lon, 0.0).to_ecef());
                                        let lb = physics.ecef_to_local(
                                            &metaverse_core::coordinates::GPS::new(seg.b_lat, seg.b_lon, 0.0).to_ecef());
                                        let pa = Vec3::new(la.x, (sa - origin_gps.alt) as f32 + 0.05, la.z);
                                        let pb = Vec3::new(lb.x, (sb - origin_gps.alt) as f32 + 0.05, lb.z);
                                        road_segments.push((pa, pb, seg.width_m, seg.road_type.clone()));
                                        if let Some(ref name) = seg.name {
                                            let mid = Vec3::new((pa.x+pb.x)*0.5, (pa.y+pb.y)*0.5+3.0, (pa.z+pb.z)*0.5);
                                            if !road_labels.iter().any(|(_, n)| n == name) {
                                                road_labels.push((mid, name.clone()));
                                            }
                                        }
                                    }
                                    osm_geom_dirty = true;
                                }
                            }
                            // Water polygons from newly loaded game-phase chunks
                            let new_ids_water: Vec<_> = chunk_streamer.newly_loaded_chunks.iter().copied().collect();
                            for chunk_id in new_ids_water {
                                let (lat_min, lat_max, lon_min, lon_max) = chunk_id.gps_bounds();
                                let osm = metaverse_core::osm::fetch_osm_for_chunk(
                                    lat_min, lat_max, lon_min, lon_max, &osm_cache_dir);
                                if !osm.water.is_empty() {
                                    for w in &osm.water {
                                        if w.polygon.len() < 3 { continue; }
                                        let key = ((w.polygon[0].lat * 1e6) as i64,
                                                   (w.polygon[0].lon * 1e6) as i64);
                                        if !water_poly_seen.insert(key) { continue; }
                                        // Per-vertex elevation — see first water block above.
                                        let verts: Vec<Vec3> = w.polygon.iter().map(|gps| {
                                            let h = physics.ecef_to_local(
                                                &metaverse_core::coordinates::GPS::new(gps.lat, gps.lon, 0.0).to_ecef());
                                            let elev = osm_elev_pipeline.query(
                                                &metaverse_core::coordinates::GPS::new(gps.lat, gps.lon, 0.0))
                                                .map(|e| e.meters)
                                                .unwrap_or(2.0);
                                            let vy = (elev - origin_gps.alt) as f32 + 0.05;
                                            Vec3::new(h.x, vy, h.z)
                                        }).collect();
                                        water_polygons.push(verts);
                                    }
                                    osm_geom_dirty = true;
                                }
                            }
                        }

                        // Always update AOI subscriptions when loaded chunks change —
                        // do NOT gate on peer_count because we need to be subscribed
                        // before the first peer connects, not after.
                        let loaded_set: std::collections::HashSet<metaverse_core::chunk::ChunkId> =
                            chunk_streamer.loaded_chunk_ids().iter().copied().collect();
                        let _ = multiplayer.update_subscribed_chunks(&loaded_set);

                        // Manifest broadcast only makes sense when peers are present
                        if multiplayer.peer_count() > 0 {
                            let new_entries: Vec<_> = chunk_streamer.newly_loaded_chunks.iter()
                                .filter_map(|id| chunk_streamer.get_chunk(id).map(|c| (*id, c.last_modified)))
                                .collect();
                            if !new_entries.is_empty() {
                                let _ = multiplayer.broadcast_chunk_manifest(new_entries);
                            }
                        }
                    }
                    
                    // Debug: Log streaming activity (not every frame, too spammy)
                    if frame_count % 120 == 0 {
                        let has_activity = chunk_streamer.stats.chunks_queued > 0 
                            || chunk_streamer.stats.chunks_loading > 0
                            || chunk_streamer.stats.chunks_loaded_this_frame > 0;
                        
                        if has_activity {
                            println!("🌍 ChunkStreamer: {} loaded, {} queued, {} loading", 
                                chunk_streamer.stats.chunks_loaded,
                                chunk_streamer.stats.chunks_queued,
                                chunk_streamer.stats.chunks_loading);
                        }
                    }
                    } // end game_mode == OpenWorld chunk streaming block
                    
                    // Update camera
                    camera.position = player.camera_position_local(&physics);
                    camera.yaw = player.camera_yaw;
                    camera.pitch = player.camera_pitch;
                    
                    // Update player hitbox transform
                    let hitbox_offset = Vec3::new(0.0, -1.6, 0.0);
                    let player_model_matrix = Mat4::from_rotation_translation(
                        glam::Quat::from_rotation_y(player.camera_yaw),
                        camera.position + hitbox_offset
                    );
                    context.queue.write_buffer(&player_model_uniform, 0, bytemuck::cast_slice(player_model_matrix.as_ref()));
                    
                    // Update crosshair
                    let crosshair_pos = camera.position + player.camera_forward() * 0.5;
                    let crosshair_matrix = Mat4::from_translation(crosshair_pos);
                    context.queue.write_buffer(&crosshair_uniform, 0, bytemuck::cast_slice(crosshair_matrix.as_ref()));
                    
                    // Update remote player transforms
                    let remote_count = multiplayer.remote_players().count();
                    for remote in multiplayer.remote_players() {
                        let transform = remote_player_transform(remote, &physics);
                        let local_pos = physics.ecef_to_local(&remote.position);
                        
                        // Debug: Log remote player rendering every 60 frames
                        if frame_count % 60 == 0 {
                            println!("🎨 Rendering remote player at Local=({:.1}, {:.1}, {:.1})", 
                                local_pos.x, local_pos.y, local_pos.z);
                        }
                        
                        // Get or create bind group for this peer
                        if !remote_player_bind_groups.contains_key(&remote.peer_id) {
                            let (uniform, bind_group) = pipeline.create_model_bind_group(&context.device, &transform);
                            remote_player_bind_groups.insert(remote.peer_id, (uniform, bind_group));
                            println!("✨ Created bind group for remote player: {}", short_peer_id(&remote.peer_id));
                        } else {
                            // Update existing transform
                            let (uniform, _) = remote_player_bind_groups.get(&remote.peer_id).unwrap();
                            context.queue.write_buffer(uniform, 0, bytemuck::cast_slice(transform.as_ref()));
                        }
                    }
                    
                    if frame_count % 60 == 0 && remote_count > 0 {
                        println!("📊 Remote players to render: {}", remote_count);
                    }
                    
                    // Regenerate dirty chunks (per-chunk, not global)
                    for chunk_data in chunk_streamer.loaded_chunks_mut() {
                        if chunk_data.dirty {
                            let min_voxel = chunk_data.id.min_voxel();
                            let max_voxel = chunk_data.id.max_voxel();
                            let (mut new_mesh, chunk_center) = extract_chunk_mesh(&chunk_data.octree, &min_voxel, &max_voxel);
                            
                            let offset = Vec3::new(
                                (chunk_center.x - origin_voxel.x) as f32,
                                (chunk_center.y - origin_voxel.y) as f32,
                                (chunk_center.z - origin_voxel.z) as f32,
                            );

                            if !new_mesh.vertices.is_empty() {
                                for vertex in &mut new_mesh.vertices {
                                    vertex.position[0] += offset.x;
                                    vertex.position[1] += offset.y;
                                    vertex.position[2] += offset.z;
                                }
                                
                                chunk_data.mesh_buffer = Some(MeshBuffer::from_mesh(&context.device, &new_mesh));
                                
                                let new_collider = metaverse_core::physics::create_collision_from_mesh(
                                    &mut physics,
                                    &new_mesh,
                                    &origin_voxel,
                                    chunk_data.collider,
                                );
                                chunk_data.collider = Some(new_collider);
                            } else {
                                chunk_data.mesh_buffer = None;
                                chunk_data.collider = None;
                            }
                            // Rebuild water surface mesh
                            let mut new_water = extract_water_surface_mesh(&chunk_data.octree, &min_voxel, &max_voxel);
                            if !new_water.vertices.is_empty() {
                                for v in &mut new_water.vertices {
                                    v.position[0] += offset.x;
                                    v.position[1] += offset.y;
                                    v.position[2] += offset.z;
                                }
                                chunk_data.water_mesh_buffer = Some(MeshBuffer::from_mesh(&context.device, &new_water));
                            } else {
                                chunk_data.water_mesh_buffer = None;
                            }
                            chunk_data.dirty = false;
                        }
                    }
                    
                    // Rebuild OSM geometry buffers when new objects arrived
                    if osm_geom_dirty {
                        let all_objs: Vec<_> = multiplayer.all_world_objects().cloned().collect();

                        // Rebuild GLB building instances (used when GLB models are loaded)
                        building_instances.clear();
                        if glb_models_loaded {
                            for obj in &all_objs {
                                let type_str = obj.object_type.as_str();
                                if !type_str.starts_with("building") { continue; }
                                let [cx, cy, cz] = obj.position;

                                // Deterministic rotation from position hash
                                let hash = ((cx * 100.0) as i32)
                                    .wrapping_mul(1664525)
                                    .wrapping_add(((cz * 100.0) as i32).wrapping_mul(1013904223));
                                let rotation_y = (hash.abs() % 360) as f32 * std::f32::consts::PI / 180.0;

                                // Scale from footprint dimensions if available
                                let cfg: serde_json::Value =
                                    serde_json::from_str(&obj.content_key).unwrap_or_default();
                                let w = cfg.get("width").and_then(|v| v.as_f64()).unwrap_or(10.0) as f32;
                                let h = cfg.get("height").and_then(|v| v.as_f64()).unwrap_or(10.0) as f32;
                                let d = cfg.get("depth").and_then(|v| v.as_f64()).unwrap_or(10.0) as f32;
                                // Kenney model actual dimensions (measured from GLB bounds):
                                // 0=commercial: 0.884×1.293×0.940 m
                                // 1=industrial: 2.084×1.470×1.242 m
                                // 2=residential: 1.300×0.834×1.028 m
                                // 3=skyscraper: 1.360×2.880×1.360 m
                                let model_idx_for_scale = building_model_idx(type_str);
                                let (mw, mh, md) = match model_idx_for_scale {
                                    0 => (0.884_f32, 1.293_f32, 0.940_f32),
                                    1 => (2.084_f32, 1.470_f32, 1.242_f32),
                                    2 => (1.300_f32, 0.834_f32, 1.028_f32),
                                    3 => (1.360_f32, 2.880_f32, 1.360_f32),
                                    _ => (1.0_f32, 1.0_f32, 1.0_f32),
                                };
                                let sx = w / mw;
                                let sy = h / mh;
                                let sz = d / md;

                                let transform = glam::Mat4::from_translation(glam::Vec3::new(cx, cy, cz))
                                    * glam::Mat4::from_rotation_y(rotation_y)
                                    * glam::Mat4::from_scale(glam::Vec3::new(sx, sy, sz));

                                let model_idx = building_model_idx(type_str);
                                if building_glb_models[model_idx].as_ref().map(|m| m.has_real_texture).unwrap_or(false) {
                                    let (_, bind_group) = pipeline.create_model_bind_group(&context.device, &transform);
                                    building_instances.push((model_idx, bind_group));
                                }
                            }
                        }

                        // Flat-colour fallback mesh: used when GLB models have no real texture
                        // (render white on user machines) or when GLB didn't load at all.
                        let use_flat_colour = building_instances.is_empty();
                        let bld_mesh = if use_flat_colour {
                            build_buildings_mesh(&all_objs)
                        } else {
                            metaverse_core::mesh::Mesh::new()
                        };
                        buildings_mesh_buffer = if bld_mesh.vertices.is_empty() { None }
                            else { Some(MeshBuffer::from_mesh(&context.device, &bld_mesh)) };
                        let rd_mesh = build_roads_mesh(&road_segments);
                        roads_mesh_buffer = if rd_mesh.vertices.is_empty() { None }
                            else { Some(MeshBuffer::from_mesh(&context.device, &rd_mesh)) };
                        // Water is now voxel-based (WATER/GRAVEL/SAND in terrain.rs).
                        // The OSM polygon overlay is disabled — terrain voxels handle the
                        // river surface following the actual slope per column.
                        water_mesh_buffer = None;
                        // Debug: build bright-yellow polygon outline mesh when O is pressed
                        osm_debug_buffer = if show_osm_debug {
                            let dbg = build_osm_debug_outlines(&water_polygons);
                            if dbg.vertices.is_empty() { None }
                            else { Some(MeshBuffer::from_mesh(&context.device, &dbg)) }
                        } else {
                            None
                        };
                        osm_geom_dirty = false;
                    }

                    // Render
                    pipeline.update_camera(&context.queue, &camera);
                    billboard_pipeline.update_camera(&context.queue, &camera.build_view_projection_matrix());

                    // Refresh billboard content every 120 frames when in Construct,
                    // but only build the billboard for the module the player is near.
                    billboard_frame_counter = billboard_frame_counter.wrapping_add(1);
                    const MODULE_SECTIONS: [Option<Section>; 6] = [
                        None,                       // 0: Login
                        None,                       // 1: Signup
                        Some(Section::Forums),      // 2: Forums
                        Some(Section::Wiki),        // 3: Wiki
                        Some(Section::Marketplace), // 4: Marketplace
                        Some(Section::Post),        // 5: Post Office
                    ];
                    if matches!(game_mode, GameMode::Construct) && billboard_frame_counter % 120 == 1 {
                        // Determine which module to (re-)build: prefer nearest to player
                        let build_idx = hud_near_module.or_else(|| {
                            // find the closest content module
                            let ploc = Vec3::new(
                                player_local_pos.x as f32,
                                player_local_pos.y as f32,
                                player_local_pos.z as f32,
                            );
                            MODULE_SECTIONS.iter().enumerate()
                                .filter(|(_, s)| s.is_some())
                                .min_by(|(i, _), (j, _)| {
                                    let di = (MODULES[*i].door_pos() - ploc).length();
                                    let dj = (MODULES[*j].door_pos() - ploc).length();
                                    di.partial_cmp(&dj).unwrap()
                                })
                                .map(|(i, _)| i)
                        });
                        if let Some(i) = build_idx {
                            if let Some(section) = &MODULE_SECTIONS[i] {
                                let items = multiplayer.get_content(section.as_str());
                                let needs = module_billboards[i]
                                    .as_ref()
                                    .map(|mb| mb.needs_rebuild(items))
                                    .unwrap_or(true);
                                if needs {
                                    module_billboards[i] = Some(ModuleBillboards::build(
                                        &context.device, &context.queue, &billboard_pipeline,
                                        section.clone(), items,
                                        MODULES[i].room_center(),
                                        MODULES[i].outward_normal(),
                                    ));
                                }
                            }
                        }
                    }

                    // Rebuild placed world-object billboards every 120 frames
                    if billboard_frame_counter % 120 == 1 {
                        use metaverse_core::world_objects::ObjectType;
                        let all_objs: Vec<_> = multiplayer.all_world_objects()
                            .filter(|o| matches!(o.object_type, ObjectType::Billboard))
                            .collect();
                        let current_ids: Vec<String> = all_objs.iter().map(|o| o.id.clone()).collect();
                        placed_billboards.retain(|(id, _)| current_ids.contains(id));
                        for obj in &all_objs {
                            if placed_billboards.iter().any(|(id, _)| id == &obj.id) {
                                continue;
                            }
                            let items = multiplayer.get_content(&obj.content_key);
                            let section = match obj.content_key.as_str() {
                                "wiki"        => metaverse_core::meshsite::Section::Wiki,
                                "marketplace" => metaverse_core::meshsite::Section::Marketplace,
                                "post"        => metaverse_core::meshsite::Section::Post,
                                _             => metaverse_core::meshsite::Section::Forums,
                            };
                            let mb = ModuleBillboards::build(
                                &context.device, &context.queue, &billboard_pipeline,
                                section, items, obj.pos_vec3(), obj.facing_normal(),
                            );
                            placed_billboards.push((obj.id.clone(), mb));
                        }
                    }
                    
                    match context.surface.get_current_texture() {
                        Ok(frame) => {
                            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                            
                            let mut encoder = context.device.create_command_encoder(
                                &wgpu::CommandEncoderDescriptor { label: Some("Render") }
                            );
                            
                            {
                                let mut render_pass = pipeline.begin_frame(&mut encoder, &view);
                                pipeline.set_pipeline(&mut render_pass);

                                // ── Render Construct scene (only when in Construct mode) ──
                                if game_mode == GameMode::Construct {
                                    construct_floor_buffer.render(&mut render_pass);
                                    construct_pillars_buffer.render(&mut render_pass);
                                    construct_terminal_buffer.render(&mut render_pass);
                                    construct_portal_buffer.render(&mut render_pass);
                                    construct_doors_buffer.render(&mut render_pass);

                                    // Render only the nearest module room's billboard
                                    if let Some(i) = hud_near_module.or_else(|| {
                                        let ploc = Vec3::new(
                                            player_local_pos.x as f32,
                                            player_local_pos.y as f32,
                                            player_local_pos.z as f32,
                                        );
                                        MODULE_SECTIONS.iter().enumerate()
                                            .filter(|(_, s)| s.is_some())
                                            .min_by(|(a, _), (b, _)| {
                                                let da = (MODULES[*a].door_pos() - ploc).length();
                                                let db = (MODULES[*b].door_pos() - ploc).length();
                                                da.partial_cmp(&db).unwrap()
                                            })
                                            .map(|(idx, _)| idx)
                                    }) {
                                        if let Some(mb) = &module_billboards[i] {
                                            billboard_pipeline.begin_render(&mut render_pass);
                                            mb.render(&mut render_pass);
                                            pipeline.set_pipeline(&mut render_pass);
                                        }
                                    }
                                }
                                
                                // Render terrain chunks (only in Open World mode)
                                if game_mode == GameMode::OpenWorld {
                                    for chunk_data in chunk_streamer.loaded_chunks() {
                                        if let Some(mesh_buffer) = &chunk_data.mesh_buffer {
                                            mesh_buffer.render(&mut render_pass);
                                        }
                                    }
                                    // Switch to OSM flat-colour pipeline for water/roads/buildings
                                    osm_pipeline.set_pipeline(&mut render_pass);
                                    render_pass.set_bind_group(0, pipeline.camera_bind_group(), &[]);
                                    render_pass.set_bind_group(1, &pipeline.model_bind_group, &[]);
                                    // Render per-chunk voxel water surfaces
                                    for chunk_data in chunk_streamer.loaded_chunks() {
                                        if let Some(buf) = &chunk_data.water_mesh_buffer {
                                            buf.render(&mut render_pass);
                                        }
                                    }
                                    // Render OSM road surfaces below buildings
                                    if let Some(buf) = &roads_mesh_buffer {
                                        buf.render(&mut render_pass);
                                    }
                                    // Render OSM building boxes (flat-colour fallback)
                                    if !building_instances.is_empty() {
                                        // Switch to textured pipeline for GLB buildings
                                        textured_pipeline.set_pipeline(&mut render_pass, pipeline.camera_bind_group());
                                        for (model_idx, model_bind_group) in &building_instances {
                                            if let Some(model) = &building_glb_models[*model_idx] {
                                                TexturedPipeline::draw_model(&mut render_pass, model, model_bind_group);
                                            }
                                        }
                                        // Restore OSM pipeline for anything that follows in this block
                                        osm_pipeline.set_pipeline(&mut render_pass);
                                        render_pass.set_bind_group(0, pipeline.camera_bind_group(), &[]);
                                        render_pass.set_bind_group(1, &pipeline.model_bind_group, &[]);
                                    } else if let Some(buf) = &buildings_mesh_buffer {
                                        buf.render(&mut render_pass);
                                    }
                                    // Restore main pipeline for anything that follows
                                    pipeline.set_pipeline(&mut render_pass);
                                    // Debug: OSM water polygon outlines (press O to toggle)
                                    if let Some(buf) = &osm_debug_buffer {
                                        osm_pipeline.set_pipeline(&mut render_pass);
                                        render_pass.set_bind_group(0, pipeline.camera_bind_group(), &[]);
                                        render_pass.set_bind_group(1, &pipeline.model_bind_group, &[]);
                                        buf.render(&mut render_pass);
                                        pipeline.set_pipeline(&mut render_pass);
                                    }
                                }

                                // Render placed world-object billboards (any game mode)
                                for (_, mb) in &placed_billboards {
                                    billboard_pipeline.begin_render(&mut render_pass);
                                    mb.render(&mut render_pass);
                                    pipeline.set_pipeline(&mut render_pass);
                                }

                                // Render terminal WORLDNET screen (always visible in Construct)
                                if matches!(game_mode, GameMode::Construct) {
                                    billboard_pipeline.begin_render(&mut render_pass);
                                    terminal_screen.render(&mut render_pass);
                                    pipeline.set_pipeline(&mut render_pass);
                                }
                                pipeline.set_model_bind_group(&mut render_pass, &player_model_bind_group);
                                hitbox_buffer.render(&mut render_pass);
                                
                                // Render all remote players
                                let mut rendered_count = 0;
                                for remote in multiplayer.remote_players() {
                                    if let Some((_, bind_group)) = remote_player_bind_groups.get(&remote.peer_id) {
                                        pipeline.set_model_bind_group(&mut render_pass, bind_group);
                                        remote_player_buffer.render(&mut render_pass);
                                        rendered_count += 1;
                                    }
                                }
                                
                                if frame_count % 60 == 0 && rendered_count > 0 {
                                    println!("🖼️  Actually rendered {} remote players", rendered_count);
                                }
                                
                                // Render crosshair (last, on top)
                                pipeline.set_model_bind_group(&mut render_pass, &crosshair_bind_group);
                                crosshair_buffer.render(&mut render_pass);
                            }
                            
                            context.queue.submit(std::iter::once(encoder.finish()));

                            // Signup overlay — rendered on top of the 3D world
                            // into the same texture view before presenting.
                            if let Some(ref mut s) = signup {
                                if let Some((key_type, name, email)) = s.render(&context, &view, &window) {
                                    // LoadKey path reuses `name` as the file path.
                                    if key_type == KeyType::User && name.as_deref().map(|p| p.contains(".key")).unwrap_or(false) {
                                        // Returning user — load key from path (future: implement file load)
                                        println!("🔑 Load key from: {}", name.as_deref().unwrap_or("?"));
                                        signup = None;
                                    } else if let Err(e) = identity.save_with_type(key_type, name, None) {
                                        eprintln!("⚠️  Failed to save identity: {}", e);
                                    } else {
                                        println!("✅ Identity saved ({:?})", key_type);
                                        if let Some(e) = email {
                                            println!("   Email (verification pending): {}", e);
                                        }
                                        signup = None;
                                    }
                                }
                            }

                            // Compose overlay — in-game content posting
                            if let Some(ref mut c) = compose {
                                match c.render(&context, &view, &window) {
                                    ComposeResult::Submit => {
                                        use metaverse_core::meshsite::ContentItem;
                                        use std::time::{SystemTime, UNIX_EPOCH};
                                        let now_ms = SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .map(|d| d.as_millis() as u64)
                                            .unwrap_or(0);
                                        let mut item = ContentItem {
                                            id: String::new(),
                                            section: c.section.clone(),
                                            title: c.title.trim().to_string(),
                                            body: c.body.trim().to_string(),
                                            author: c.author.clone(),
                                            signature: vec![],
                                            created_at: now_ms,
                                        };
                                        item.id = item.compute_id();
                                        multiplayer.publish_content(&item);
                                        println!("📤 Published [{:?}] \"{}\"", item.section, item.title);
                                        // Trigger immediate billboard refresh so post appears on wall
                                        billboard_frame_counter = 0;
                                        compose = None;
                                        // Restore mouse grab
                                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Locked);
                                        window.set_cursor_visible(false);
                                        cursor_grabbed = true;
                                    }
                                    ComposeResult::Cancel => {
                                        compose = None;
                                        // Restore mouse grab
                                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Locked);
                                        window.set_cursor_visible(false);
                                        cursor_grabbed = true;
                                    }
                                    ComposeResult::Continue => {}
                                }
                            }

                            // Placement overlay — shown after compose, before HUD
                            if let Some(ref mut p) = placement {
                                let done = p.render(&context, &view, &window, &server_url);
                                if done {
                                    placement = None;
                                    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Locked);
                                    window.set_cursor_visible(false);
                                    cursor_grabbed = true;
                                    // Trigger world objects refresh next frame
                                    last_world_obj_chunk = None;
                                }
                            }

                            // Debug HUD — always visible, top-left corner
                            let mode_str = match &game_mode {
                                GameMode::Construct => "Construct",
                                GameMode::OpenWorld => "Open World",
                            };
                            // Only show module proximity prompt when in Construct walking mode
                            let near_module_hud = if matches!(game_mode, GameMode::Construct) {
                                hud_near_module
                            } else {
                                None
                            };
                            hud.render(
                                &context, &view, &window,
                                mode_str, {
                                    let g = player.position.to_gps();
                                    (g.lat, g.lon, g.alt)
                                },
                                player.camera_yaw,
                                dist_portal, dist_terminal,
                                near_portal, hud_near_terminal,
                                near_module_hud,
                                &road_labels,
                                camera.build_view_projection_matrix(),
                            );

                            frame.present();
                        }
                        Err(e) => eprintln!("Surface error: {:?}", e),
                    }
                    
                    // FPS counter and stats
                    frame_count += 1;
                    if fps_timer.elapsed().as_secs() >= 1 {
                        let peer_count = multiplayer.peer_count();
                        
                        println!("FPS: {} | Peers: {} | Local: ({:.1}, {:.1}, {:.1}) | Mode: {:?}",
                            frame_count,
                            peer_count,
                            player_local_pos.x,
                            player_local_pos.y,
                            player_local_pos.z,
                            player_mode,
                        );
                        
                        frame_count = 0;
                        fps_timer = Instant::now();
                    }
                    
                    // Print detailed stats every 5 seconds
                    if last_stats_print.elapsed().as_secs() >= 5 {
                        let stats = multiplayer.stats();
                        let peer_count = multiplayer.peer_count();
                        
                        if peer_count > 0 {
                            println!("\n📊 Network Statistics:");
                            println!("   Connected peers: {}", peer_count);
                            println!("   Player states: sent={}, received={}", 
                                stats.player_states_sent, stats.player_states_received);
                            println!("   Voxel ops: sent={}, received={}, applied={}, rejected={}", 
                                stats.voxel_ops_sent, stats.voxel_ops_received,
                                stats.voxel_ops_applied, stats.voxel_ops_rejected);
                            println!("   Invalid signatures: {}", stats.invalid_signatures);
                            println!("   Total messages: {}\n", stats.messages_received);
                        }
                        last_stats_print = Instant::now();
                    }
                }
                
                _ => {}
            }
            
            Event::DeviceEvent { event, .. } => {
                if cursor_grabbed {
                    if let DeviceEvent::MouseMotion { delta } = event {
                        player.camera_yaw += (delta.0 as f32) * 0.002;
                        player.camera_pitch -= (delta.1 as f32) * 0.002;
                        player.camera_pitch = player.camera_pitch.clamp(-1.5, 1.5);
                    }
                }
            }
            
            Event::AboutToWait => {
                window.request_redraw();
            }
            
            _ => {}
        }
    }) as GameHandlerFn
    })),
};
    event_loop.run_app(&mut app).unwrap();
}

/// Create local player cube (green)
fn create_local_player_cube() -> Mesh {
    let w = 0.3;
    let h = 0.9;
    let mut mesh = Mesh::new();
    
    // Green color for local player
    let color = Vec3::new(0.3, 1.0, 0.3);
    
    // Bottom face
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h, -w), color));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h, -w), color));
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h,  w), color));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h,  w), color));
    
    // Top face
    let v4 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h, -w), color));
    let v5 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h, -w), color));
    let v6 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h,  w), color));
    let v7 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h,  w), color));
    
    // Wireframe edges
    mesh.add_line(v0, v1); mesh.add_line(v1, v2); mesh.add_line(v2, v3); mesh.add_line(v3, v0);
    mesh.add_line(v4, v5); mesh.add_line(v5, v6); mesh.add_line(v6, v7); mesh.add_line(v7, v4);
    mesh.add_line(v0, v4); mesh.add_line(v1, v5); mesh.add_line(v2, v6); mesh.add_line(v3, v7);
    
    mesh
}

/// Create hitbox wireframe (same as phase1_week1)
fn create_hitbox_wireframe() -> Mesh {
    create_local_player_cube() // Same dimensions, reuse
}

/// Create crosshair (same as phase1_week1)
fn create_crosshair() -> Mesh {
    let mut mesh = Mesh::new();
    let size = 0.02;
    let color = Vec3::new(1.0, 1.0, 1.0);
    
    // Horizontal line
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(-size, 0.0, 0.0), color));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new( size, 0.0, 0.0), color));
    mesh.add_line(v0, v1);
    
    // Vertical line
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new(0.0, -size, 0.0), color));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(0.0,  size, 0.0), color));
    mesh.add_line(v2, v3);
    
    mesh
}

// ─── Loading screen ──────────────────────────────────────────────────────────


/// Build a triangle mesh of coloured boxes for all registered building objects.
///
/// Each building's config JSON contains `height`, `width`, `depth`.
/// Winding: CCW from outside, matching the pipeline `front_face: Ccw` setting.
fn build_buildings_mesh(
    objects: &[metaverse_core::world_objects::PlacedObject],
) -> Mesh {
    let mut mesh = Mesh::new();

    for obj in objects {
        let type_str = obj.object_type.as_str();
        if !type_str.starts_with("building") { continue; }

        // Parse footprint dimensions from config JSON
        let cfg: serde_json::Value = serde_json::from_str(&obj.content_key).unwrap_or_default();
        let h = cfg.get("height").and_then(|v| v.as_f64()).unwrap_or(10.5) as f32;
        let w = cfg.get("width").and_then(|v| v.as_f64()).unwrap_or(10.0) as f32;
        let d = cfg.get("depth").and_then(|v| v.as_f64()).unwrap_or(10.0) as f32;

        let color = building_color(type_str);
        let [cx, cy, cz] = obj.position;
        let hw = w * 0.5;
        let hd = d * 0.5;

        // 8 corners of the box
        let bot = cy;
        let top = cy + h;

        // bottom face vertices (y = bot)
        let b0 = Vec3::new(cx - hw, bot, cz - hd);
        let b1 = Vec3::new(cx + hw, bot, cz - hd);
        let b2 = Vec3::new(cx + hw, bot, cz + hd);
        let b3 = Vec3::new(cx - hw, bot, cz + hd);
        // top face vertices (y = top)
        let t0 = Vec3::new(cx - hw, top, cz - hd);
        let t1 = Vec3::new(cx + hw, top, cz - hd);
        let t2 = Vec3::new(cx + hw, top, cz + hd);
        let t3 = Vec3::new(cx - hw, top, cz + hd);

        // Top face — CCW from above (normal +Y)
        let vt0 = mesh.add_vertex(Vertex::new(t0, color));
        let vt1 = mesh.add_vertex(Vertex::new(t1, color));
        let vt2 = mesh.add_vertex(Vertex::new(t2, color));
        let vt3 = mesh.add_vertex(Vertex::new(t3, color));
        mesh.add_triangle(Triangle::new(vt0, vt2, vt1));
        mesh.add_triangle(Triangle::new(vt0, vt3, vt2));

        // Four side faces — each CCW from outside
        // North face (z = cz - hd, normal -Z)
        let vn0 = mesh.add_vertex(Vertex::new(b1, color));
        let vn1 = mesh.add_vertex(Vertex::new(b0, color));
        let vn2 = mesh.add_vertex(Vertex::new(t0, color));
        let vn3 = mesh.add_vertex(Vertex::new(t1, color));
        mesh.add_triangle(Triangle::new(vn0, vn1, vn2));
        mesh.add_triangle(Triangle::new(vn0, vn2, vn3));

        // South face (z = cz + hd, normal +Z)
        let vs0 = mesh.add_vertex(Vertex::new(b3, color));
        let vs1 = mesh.add_vertex(Vertex::new(b2, color));
        let vs2 = mesh.add_vertex(Vertex::new(t2, color));
        let vs3 = mesh.add_vertex(Vertex::new(t3, color));
        mesh.add_triangle(Triangle::new(vs0, vs1, vs2));
        mesh.add_triangle(Triangle::new(vs0, vs2, vs3));

        // East face (x = cx + hw, normal +X)
        let ve0 = mesh.add_vertex(Vertex::new(b2, color));
        let ve1 = mesh.add_vertex(Vertex::new(b1, color));
        let ve2 = mesh.add_vertex(Vertex::new(t1, color));
        let ve3 = mesh.add_vertex(Vertex::new(t2, color));
        mesh.add_triangle(Triangle::new(ve0, ve1, ve2));
        mesh.add_triangle(Triangle::new(ve0, ve2, ve3));

        // West face (x = cx - hw, normal -X)
        let vw0 = mesh.add_vertex(Vertex::new(b0, color));
        let vw1 = mesh.add_vertex(Vertex::new(b3, color));
        let vw2 = mesh.add_vertex(Vertex::new(t3, color));
        let vw3 = mesh.add_vertex(Vertex::new(t0, color));
        mesh.add_triangle(Triangle::new(vw0, vw1, vw2));
        mesh.add_triangle(Triangle::new(vw0, vw2, vw3));
    }

    mesh
}

/// Road surface colour by type.
fn road_color(road_type: &metaverse_core::osm::RoadType) -> Vec3 {
    use metaverse_core::osm::RoadType;
    match road_type {
        RoadType::Motorway | RoadType::Trunk => Vec3::new(0.9, 0.5, 0.1),
        RoadType::Primary | RoadType::Secondary => Vec3::new(0.85, 0.75, 0.2),
        RoadType::Tertiary | RoadType::Residential => Vec3::new(0.55, 0.55, 0.55),
        _ => Vec3::new(0.45, 0.45, 0.45),
    }
}

/// Build colour for building type.
fn building_color(type_str: &str) -> Vec3 {
    match type_str {
        s if s.contains("commercial") || s.contains("retail") => Vec3::new(0.4, 0.5, 0.85),
        s if s.contains("industrial") || s.contains("warehouse") => Vec3::new(0.55, 0.45, 0.35),
        s if s.contains("residential") || s.contains("house") => Vec3::new(0.85, 0.75, 0.55),
        _ => Vec3::new(0.7, 0.68, 0.62),
    }
}

fn building_model_idx(type_str: &str) -> usize {
    if type_str.contains("commercial") || type_str.contains("retail") { 0 }
    else if type_str.contains("industrial") || type_str.contains("warehouse") { 1 }
    else if type_str.contains("residential") || type_str.contains("house") || type_str.contains("suburb") { 2 }
    else if type_str.contains("skyscraper") || type_str.contains("highrise") || type_str.contains("tower") { 3 }
    else { 4 }
}

/// Build a road surface mesh from road segment data stored in placed objects.
///
/// Roads are stored as `Custom("road_*")` objects whose config JSON has
/// `a` and `b` endpoint positions and `width`.  Each segment becomes a
/// flat quad lying on the ground.
fn build_roads_mesh(road_segments: &[(Vec3, Vec3, f32, metaverse_core::osm::RoadType)]) -> Mesh {
    let mut mesh = Mesh::new();

    for (a, b, width, road_type) in road_segments {
        let color = road_color(road_type);
        let hw = width * 0.5;

        // Perpendicular to road in XZ plane
        let dir = (*b - *a).normalize();
        let perp = Vec3::new(-dir.z, 0.0, dir.x) * hw;

        let v0 = *a - perp;
        let v1 = *a + perp;
        let v2 = *b + perp;
        let v3 = *b - perp;

        // Quad lying flat — CCW from above (+Y normal)
        let i0 = mesh.add_vertex(Vertex::new(v0, color));
        let i1 = mesh.add_vertex(Vertex::new(v1, color));
        let i2 = mesh.add_vertex(Vertex::new(v2, color));
        let i3 = mesh.add_vertex(Vertex::new(v3, color));
        // CCW from above: normal = +Y (visible looking down at road surface)
        mesh.add_triangle(Triangle::new(i0, i1, i2));
        mesh.add_triangle(Triangle::new(i0, i2, i3));
    }

    mesh
}

/// Build a water surface mesh from OSM water polygons.
///
/// Each polygon is triangulated as a fan from its centroid.
/// Vertex Y values are per-vertex SRTM elevations so the surface
/// correctly follows the river gradient (e.g., 15m→5m over 15km
/// for the Brisbane CBD reach). Both faces rendered so water is
/// visible from bank level looking across.
fn build_water_mesh(water_polygons: &[Vec<Vec3>]) -> Mesh {
    let mut mesh = Mesh::new();
    let color = Vec3::new(0.18, 0.42, 0.72); // river blue

    for poly in water_polygons {
        if poly.len() < 3 { continue; }
        let sum = poly.iter().fold(Vec3::ZERO, |acc, v| acc + *v);
        let centroid = sum / poly.len() as f32;
        let n = poly.len();
        for i in 0..n {
            let next = (i + 1) % n;
            let ci = mesh.add_vertex(Vertex::new(centroid, color));
            let ai = mesh.add_vertex(Vertex::new(poly[i], color));
            let bi = mesh.add_vertex(Vertex::new(poly[next], color));
            // CCW from above (visible looking down)
            mesh.add_triangle(Triangle::new(ci, ai, bi));
            // CW — visible from bank level / below
            mesh.add_triangle(Triangle::new(ci, bi, ai));
        }
    }

    mesh
}

/// Build bright-yellow outline mesh of OSM water polygon edges (debug overlay).
/// Press O to toggle. Shows exactly where OSM polygon coverage exists.
fn build_osm_debug_outlines(water_polygons: &[Vec<Vec3>]) -> Mesh {
    let mut mesh = Mesh::new();
    let yellow = Vec3::new(1.0, 1.0, 0.0);
    for poly in water_polygons {
        if poly.len() < 2 { continue; }
        let n = poly.len();
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            // Raise slightly above terrain so lines are visible
            let a_up = Vec3::new(a.x, a.y + 0.5, a.z);
            let b_up = Vec3::new(b.x, b.y + 0.5, b.z);
            let va = mesh.add_vertex(Vertex::new(a_up, yellow));
            let vb = mesh.add_vertex(Vertex::new(b_up, yellow));
            mesh.add_line(va, vb);
        }
    }
    mesh
}
