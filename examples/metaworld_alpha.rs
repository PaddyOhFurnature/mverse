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
//! - Backquote - Cycle observability HUD detail
//! - F9 - Cycle active chunk layer view
//! - F10 - Dump active chunk stats to console
//! - F12 - Take screenshot
//! - Console shows connection events and sync statistics

use egui_wgpu::ScreenDescriptor;
use glam::{Mat4, Vec3};
use metaverse_core::{
    billboard::{BillboardPipeline, ModuleBillboards, TerminalScreen},
    chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z, ChunkId},
    chunk_manager::ChunkManager,
    chunk_streaming::{ChunkLoadState, ChunkStreamer, ChunkStreamerConfig, LoadedChunk},
    construct::{
        ConstructScene, INTERACT_RADIUS, MODULE_DOOR_RADIUS, MODULES, SIGNUP_TERMINAL_POS,
        WORLD_PORTAL_POS,
    },
    coordinates::{ECEF, GPS},
    elevation::{
        CopernicusElevationSource, ElevationPipeline, OpenTopographySource, P2PElevationSource,
        SkadiElevationSource,
    },
    identity::{Identity, KeyType},
    marching_cubes::{extract_chunk_mesh, extract_chunk_mesh_smooth, extract_water_surface_mesh},
    materials::MaterialId,
    mesh::{Mesh, Vertex},
    meshsite::Section,
    messages::{Material, MovementMode},
    multiplayer::MultiplayerSystem,
    osm::{OsmDiskCache, fetch_osm_for_chunk_with_cache},
    physics::{PHYSICS_TIMESTEP, PhysicsWorld, Player},
    player_persistence::PlayerPersistence,
    remote_render::{create_remote_player_capsule, remote_player_transform, short_peer_id},
    renderer::{
        Camera, GlbModel, MeshBuffer, OsmPipeline, RenderContext, RenderPipeline, TexturedPipeline,
        WaterPipeline,
    },
    terrain::TerrainGenerator,
    user_content::UserContentLayer,
    vector_clock::VectorClock,
    voxel::VoxelCoord,
    world_inference, worldgen_river,
};
use rapier3d::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
};

// Minimal ApplicationHandler wrapper — defers window creation to resumed(), then
// dispatches all events to the existing game-loop closure unchanged.
type GameHandlerFn = Box<dyn FnMut(Event<()>, &ActiveEventLoop)>;
type InitFn = Box<dyn FnOnce(&ActiveEventLoop) -> GameHandlerFn>;
struct GameApp {
    init: Option<InitFn>,
    handler: Option<GameHandlerFn>,
}
impl ApplicationHandler for GameApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if let Some(f) = self.init.take() {
            self.handler = Some(f(el));
        }
    }
    fn window_event(
        &mut self,
        el: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let Some(h) = &mut self.handler {
            h(Event::WindowEvent { window_id, event }, el);
        }
    }
    fn device_event(&mut self, el: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
        if let Some(h) = &mut self.handler {
            h(Event::DeviceEvent { device_id, event }, el);
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(h) = &mut self.handler {
            h(Event::AboutToWait, el);
        }
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
    CreateUser {
        name: String,
    },
    /// New Guest: email + nickname
    CreateGuest {
        email: String,
        nick: String,
    },
    /// Returning user: path to key file
    LoadKey {
        path: String,
        error: Option<String>,
    },
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
        let egui_renderer =
            egui_wgpu::Renderer::new(&context.device, context.config.format, None, 1, false);
        Self {
            step: SignupStep::Choosing,
            egui_ctx,
            egui_state,
            egui_renderer,
        }
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

        self.egui_state
            .handle_platform_output(window, full_output.platform_output);

        let tris = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&context.device, &context.queue, *id, delta);
        }
        let screen_desc = ScreenDescriptor {
            size_in_pixels: [context.config.width, context.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egui_signup"),
            });
        self.egui_renderer.update_buffers(
            &context.device,
            &context.queue,
            &mut encoder,
            &tris,
            &screen_desc,
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
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    section: metaverse_core::meshsite::Section,
    title: String,
    body: String,
    author: String,
    error: Option<String>,
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
            egui_ctx.clone(),
            egui_ctx.viewport_id(),
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer =
            egui_wgpu::Renderer::new(&context.device, context.config.format, None, 1, false);
        Self {
            egui_ctx,
            egui_state,
            egui_renderer,
            section,
            title: String::new(),
            body: String::new(),
            author,
            error: None,
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
        let title = &mut self.title;
        let body = &mut self.body;
        let error = &mut self.error;

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            // dim backdrop
            egui::Area::new(egui::Id::new("compose_backdrop"))
                .fixed_pos(egui::pos2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        ctx.screen_rect(),
                        0.0,
                        egui::Color32::from_black_alpha(190),
                    );
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
                        for s in [
                            Section::Forums,
                            Section::Wiki,
                            Section::Marketplace,
                            Section::Post,
                        ] {
                            let active =
                                std::mem::discriminant(section) == std::mem::discriminant(&s);
                            let label = s.as_str();
                            if ui.selectable_label(active, label).clicked() {
                                *section = s;
                            }
                        }
                    });
                    ui.add_space(6.0);

                    ui.label("Title:");
                    ui.add(
                        egui::TextEdit::singleline(title)
                            .desired_width(f32::INFINITY)
                            .hint_text("Subject / title (required)"),
                    );
                    ui.add_space(6.0);

                    ui.label("Body:");
                    ui.add(
                        egui::TextEdit::multiline(body)
                            .desired_width(f32::INFINITY)
                            .desired_rows(10)
                            .hint_text("Write your post here..."),
                    );
                    ui.add_space(8.0);

                    if let Some(e) = error.as_deref() {
                        ui.colored_label(egui::Color32::from_rgb(255, 100, 100), e);
                        ui.add_space(4.0);
                    }

                    ui.horizontal(|ui| {
                        let can_submit = !title.trim().is_empty() && !body.trim().is_empty();
                        if ui
                            .add_enabled(can_submit, egui::Button::new("📤  Post"))
                            .clicked()
                        {
                            result = ComposeResult::Submit;
                        }
                        if ui.button("✖  Cancel").clicked() {
                            result = ComposeResult::Cancel;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!("[ ESC to cancel ]"))
                                    .color(egui::Color32::DARK_GRAY)
                                    .size(11.0),
                            );
                        });
                    });
                });
        });

        self.egui_state
            .handle_platform_output(window, full_output.platform_output);
        let tris = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&context.device, &context.queue, *id, delta);
        }
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [context.config.width, context.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("compose"),
            });
        self.egui_renderer.update_buffers(
            &context.device,
            &context.queue,
            &mut encoder,
            &tris,
            &screen_desc,
        );
        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("compose_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
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
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    obj_type: String,
    content_key: String,
    label: String,
    /// World position where the object will be placed (set on open).
    position: [f32; 3],
    /// Player yaw at time of open — object faces toward player.
    rotation_y: f32,
    placed_by: String,
    status: Option<String>,
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
            egui_ctx.clone(),
            egui_ctx.viewport_id(),
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer =
            egui_wgpu::Renderer::new(&context.device, context.config.format, None, 1, false);
        Self {
            egui_ctx,
            egui_state,
            egui_renderer,
            obj_type: "Billboard".to_string(),
            content_key: "forums".to_string(),
            label: String::new(),
            position,
            rotation_y,
            placed_by,
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

        let obj_type = &mut self.obj_type;
        let content_key = &mut self.content_key;
        let label = &mut self.label;
        let status = &mut self.status;
        let position = self.position;
        let rotation_y = self.rotation_y;
        let placed_by = self.placed_by.clone();
        let server = server_url.to_string();

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
        let tris = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&context.device, &context.queue, *id, delta);
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [context.config.width, context.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("placement_overlay"),
            });
        self.egui_renderer.update_buffers(
            &context.device,
            &context.queue,
            &mut encoder,
            &tris,
            &screen,
        );
        {
            let rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("placement_overlay_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut rp = rp.forget_lifetime();
            self.egui_renderer.render(&mut rp, &tris, &screen);
        }
        context.queue.submit(std::iter::once(encoder.finish()));
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        self.egui_state
            .handle_platform_output(window, full_output.platform_output);

        close
    }
}

struct DebugHud {
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

impl DebugHud {
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
        let egui_renderer =
            egui_wgpu::Renderer::new(&context.device, context.config.format, None, 1, false);
        Self {
            egui_ctx,
            egui_state,
            egui_renderer,
        }
    }

    fn on_event(&mut self, window: &winit::window::Window, event: &WindowEvent) -> bool {
        self.egui_state.on_window_event(window, event).consumed
    }

    fn render(
        &mut self,
        context: &RenderContext,
        view: &wgpu::TextureView,
        window: &winit::window::Window,
        show_basic_hud: bool,
        // Data to display
        game_mode: &str,
        gps: (f64, f64, f64), // (lat, lon, orthometric_alt_m)
        bearing_deg: f32,     // geographic bearing derived from forward direction
        dist_portal: f32,
        dist_terminal: f32,
        near_portal: bool,
        near_terminal: bool,
        near_module: Option<usize>,
        observability_mode: ObservabilityMode,
        probe: Option<&ObservabilityProbe>,
        layer_view_mode: LayerViewMode,
        layer_summary: Option<&ChunkLayerSummary>,
    ) {
        let compass = match bearing_deg as u32 {
            0..=22 | 338..=360 => "N",
            23..=67 => "NE",
            68..=112 => "E",
            113..=157 => "SE",
            158..=202 => "S",
            203..=247 => "SW",
            248..=292 => "W",
            293..=337 => "NW",
            _ => "N",
        };

        let raw_input = self.egui_state.take_egui_input(window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            if show_basic_hud {
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
                                    format!("GPS  {:+.5}°  {:+.5}°", gps.0, gps.1))
                                    .color(egui::Color32::from_rgb(120, 220, 255)).size(13.0).strong());
                                ui.label(egui::RichText::new(
                                    format!("Alt MSL: {:.1}m", gps.2))
                                    .color(egui::Color32::LIGHT_GRAY).size(12.0));
                                ui.separator();
                                ui.label(egui::RichText::new(
                                    format!("Observability: {}", observability_mode.label()))
                                    .color(egui::Color32::from_rgb(255, 220, 80)).size(12.0));

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

                                if observability_mode != ObservabilityMode::Basic {
                                    ui.separator();
                                    if let Some(probe) = probe {
                                        ui.monospace(format!(
                                            "Chunk: ({}, {}, {})",
                                            probe.player_chunk.x, probe.player_chunk.y, probe.player_chunk.z,
                                        ));
                                        ui.monospace(format!(
                                            "Voxel: ({}, {}, {})",
                                            probe.player_voxel.x, probe.player_voxel.y, probe.player_voxel.z,
                                        ));
                                        ui.monospace(format!(
                                            "Local: ({:.1}, {:.1}, {:.1})",
                                            probe.player_local.x, probe.player_local.y, probe.player_local.z,
                                        ));
                                        ui.monospace(format!(
                                            "Loaded={}  Queued={}  Loading={}",
                                            probe.loaded_chunks, probe.chunks_queued, probe.chunks_loading,
                                        ));

                                        if let Some(state) = probe.chunk_state {
                                            ui.monospace(format!(
                                                "Active: {:?}  LOD={}  Dirty={}  Dist={:.1}m",
                                                state,
                                                probe.chunk_lod.unwrap_or(0),
                                                probe.chunk_dirty.unwrap_or(false),
                                                probe.chunk_distance_m.unwrap_or(0.0),
                                            ));
                                        } else {
                                            ui.monospace("Active: chunk not loaded");
                                        }

                                        let top_text = match (probe.top_y, probe.top_material) {
                                            (Some(y), Some(mat)) => format!("Top: y={} {:?}", y, mat),
                                            _ => "Top: none".to_string(),
                                        };
                                        ui.monospace(top_text);

                                        let ground_text = match (probe.ground_y, probe.ground_material, probe.ground_clearance_voxels) {
                                            (Some(y), Some(mat), Some(clearance)) => {
                                                format!("Ground: y={} {:?}  clearance={:+}", y, mat, clearance)
                                            }
                                            (Some(y), Some(mat), None) => format!("Ground: y={} {:?}", y, mat),
                                            _ => "Ground: none".to_string(),
                                        };
                                        ui.monospace(ground_text);

                                        ui.monospace(format!(
                                            "Column: below={:?} here={:?} above={:?}",
                                            probe.below_material, probe.current_material, probe.above_material,
                                        ));

                                        if observability_mode == ObservabilityMode::Chunk {
                                            if let Some(surface_y_f) = probe.surface_y_f {
                                                ui.monospace(format!("Surface cache y={:.2}", surface_y_f));
                                            }
                                            if let Some(last_modified) = probe.chunk_last_modified {
                                                ui.monospace(format!(
                                                    "Chunk modified={}  loaded_this_frame={}",
                                                    last_modified, probe.loaded_this_frame,
                                                ));
                                            } else {
                                                ui.monospace(format!(
                                                    "loaded_this_frame={}",
                                                    probe.loaded_this_frame,
                                                ));
                                            }
                                            ui.small("F10 dumps active chunk histogram to console");
                                        }
                                    } else {
                                        ui.monospace("Probe unavailable outside Open World");
                                    }
                                    ui.small("Backquote cycles: Basic -> Probe -> Chunk");
                                }
                            });
                    });
            }

            // Compass rose — top-right corner
            if show_basic_hud {
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

            if layer_view_mode != LayerViewMode::Off {
                let panel_y = (ctx.screen_rect().height() - 290.0).max(8.0);
                egui::Area::new(egui::Id::new("layer_view_panel"))
                    .fixed_pos(egui::pos2(8.0, panel_y))
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_black_alpha(180))
                            .inner_margin(egui::Margin::same(6))
                            .corner_radius(4.0)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(
                                    format!("Layer view: {}", layer_view_mode.label()))
                                    .color(egui::Color32::from_rgb(255, 220, 80))
                                    .size(13.0).strong());
                                ui.small("F9 cycles Off -> Height -> Slope -> Ground -> Hydro -> Roads -> Buildings");

                                if let Some(summary) = layer_summary {
                                    ui.monospace(format!(
                                        "Chunk ({}, {}, {})",
                                        summary.chunk_id.x, summary.chunk_id.y, summary.chunk_id.z,
                                    ));

                                    let grid_side = 180.0;
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(grid_side, grid_side),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter_at(rect);
                                    let cell_w = rect.width() / CHUNK_SIZE_X as f32;
                                    let cell_h = rect.height() / CHUNK_SIZE_Z as f32;

                                    for z in 0..CHUNK_SIZE_Z as usize {
                                        for x in 0..CHUNK_SIZE_X as usize {
                                            let cell = &summary.columns[chunk_layer_index(x, z)];
                                            let color = layer_cell_color(cell, summary, layer_view_mode);
                                            let cell_rect = egui::Rect::from_min_size(
                                                egui::pos2(
                                                    rect.min.x + x as f32 * cell_w,
                                                    rect.min.y + z as f32 * cell_h,
                                                ),
                                                egui::vec2(cell_w.max(1.0), cell_h.max(1.0)),
                                            );
                                            painter.rect_filled(cell_rect, 0.0, color);
                                        }
                                    }
                                    painter.rect_stroke(
                                        rect,
                                        0.0,
                                        egui::Stroke::new(1.0, egui::Color32::from_gray(140)),
                                        egui::StrokeKind::Outside,
                                    );

                                    ui.monospace(format!(
                                        "columns={} water={} roads={} buildings={}",
                                        summary.non_air_columns,
                                        summary.water_columns,
                                        summary.road_columns,
                                        summary.building_columns,
                                    ));
                                    if let (Some(min_y), Some(max_y)) = (summary.min_ground_y, summary.max_ground_y) {
                                        ui.monospace(format!("ground y range: {}..{}", min_y, max_y));
                                    }

                                    if let Some(pos) = response.hover_pos() {
                                        let rel_x = ((pos.x - rect.min.x) / cell_w).floor() as i32;
                                        let rel_z = ((pos.y - rect.min.y) / cell_h).floor() as i32;
                                        if rel_x >= 0 && rel_x < CHUNK_SIZE_X as i32 && rel_z >= 0 && rel_z < CHUNK_SIZE_Z as i32 {
                                            let rel_x = rel_x as usize;
                                            let rel_z = rel_z as usize;
                                            let cell = &summary.columns[chunk_layer_index(rel_x, rel_z)];
                                            let world_vx = summary.min_voxel_x + rel_x as i64;
                                            let world_vz = summary.min_voxel_z + rel_z as i64;
                                            ui.separator();
                                            ui.monospace(format!(
                                                "hover vx={} vz={} top={:?}@{:?} ground={:?}@{:?}",
                                                world_vx,
                                                world_vz,
                                                cell.top_material,
                                                cell.top_y,
                                                cell.ground_material,
                                                cell.ground_y,
                                            ));
                                            ui.monospace(format!(
                                                "water={} road={:?} building={}",
                                                cell.has_water,
                                                cell.road_surface,
                                                cell.has_building,
                                            ));
                                            if let Some(slope) = cell.local_slope_deg {
                                                ui.monospace(format!(
                                                    "local slope={:.1}° relief≈{:.1}m",
                                                    slope,
                                                    cell.local_relief_m.unwrap_or(0.0),
                                                ));
                                            }
                                        }
                                    }
                                } else {
                                    ui.monospace("Active chunk layer data unavailable");
                                }
                            });
                    });
            }

            // Road name labels — projected from 3D world space to screen
        });

        self.egui_state
            .handle_platform_output(window, full_output.platform_output);
        let tris = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&context.device, &context.queue, *id, delta);
        }
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [context.config.width, context.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hud") });
        self.egui_renderer.update_buffers(
            &context.device,
            &context.queue,
            &mut encoder,
            &tris,
            &screen_desc,
        );
        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("hud_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut rpass, &tris, &screen_desc);
        }
        context.queue.submit(std::iter::once(encoder.finish()));
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservabilityMode {
    Basic,
    Probe,
    Chunk,
}

impl ObservabilityMode {
    fn next(self) -> Self {
        match self {
            Self::Basic => Self::Probe,
            Self::Probe => Self::Chunk,
            Self::Chunk => Self::Basic,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Probe => "Probe",
            Self::Chunk => "Chunk",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerViewMode {
    Off,
    Height,
    Slope,
    Ground,
    Hydro,
    Roads,
    Buildings,
}

impl LayerViewMode {
    fn next(self) -> Self {
        match self {
            Self::Off => Self::Height,
            Self::Height => Self::Slope,
            Self::Slope => Self::Ground,
            Self::Ground => Self::Hydro,
            Self::Hydro => Self::Roads,
            Self::Roads => Self::Buildings,
            Self::Buildings => Self::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Height => "Height",
            Self::Slope => "Slope",
            Self::Ground => "Ground",
            Self::Hydro => "Hydro",
            Self::Roads => "Roads",
            Self::Buildings => "Buildings",
        }
    }
}

#[derive(Debug, Clone)]
struct ObservabilityProbe {
    player_chunk: ChunkId,
    player_voxel: VoxelCoord,
    player_local: Vec3,
    loaded_chunks: usize,
    chunks_queued: usize,
    chunks_loading: usize,
    loaded_this_frame: usize,
    chunk_state: Option<ChunkLoadState>,
    chunk_distance_m: Option<f64>,
    chunk_lod: Option<u8>,
    chunk_dirty: Option<bool>,
    chunk_last_modified: Option<u64>,
    surface_y_f: Option<f64>,
    top_y: Option<i64>,
    top_material: Option<MaterialId>,
    ground_y: Option<i64>,
    ground_material: Option<MaterialId>,
    ground_clearance_voxels: Option<i64>,
    current_material: MaterialId,
    above_material: MaterialId,
    below_material: MaterialId,
}

#[derive(Debug, Clone, Copy)]
struct ColumnLayerInfo {
    top_y: Option<i64>,
    top_material: Option<MaterialId>,
    ground_y: Option<i64>,
    ground_material: Option<MaterialId>,
    local_slope_deg: Option<f32>,
    local_relief_m: Option<f32>,
    has_water: bool,
    road_surface: Option<MaterialId>,
    has_building: bool,
}

#[derive(Debug, Clone)]
struct ChunkLayerSummary {
    chunk_id: ChunkId,
    last_modified: u64,
    min_voxel_x: i64,
    min_voxel_z: i64,
    min_ground_y: Option<i64>,
    max_ground_y: Option<i64>,
    non_air_columns: usize,
    water_columns: usize,
    road_columns: usize,
    building_columns: usize,
    columns: Vec<ColumnLayerInfo>,
}

fn top_non_air_in_column(chunk: &LoadedChunk, vx: i64, vz: i64) -> Option<(i64, MaterialId)> {
    let min_v = chunk.id.min_voxel();
    let max_v = chunk.id.max_voxel();
    for vy in (min_v.y..max_v.y).rev() {
        let mat = chunk.octree.get_voxel(VoxelCoord::new(vx, vy, vz));
        if mat != MaterialId::AIR {
            return Some((vy, mat));
        }
    }
    None
}

fn ground_surface_in_column(chunk: &LoadedChunk, vx: i64, vz: i64) -> Option<(i64, MaterialId)> {
    let min_v = chunk.id.min_voxel();
    let max_v = chunk.id.max_voxel();
    for vy in (min_v.y..max_v.y).rev() {
        let mat = chunk.octree.get_voxel(VoxelCoord::new(vx, vy, vz));
        if matches!(
            mat,
            MaterialId::AIR | MaterialId::WATER | MaterialId::LEAVES | MaterialId::WOOD
        ) {
            continue;
        }
        return Some((vy, mat));
    }
    None
}

fn chunk_layer_index(x: usize, z: usize) -> usize {
    z * CHUNK_SIZE_X as usize + x
}

fn is_road_surface(mat: MaterialId) -> bool {
    matches!(mat, MaterialId::ASPHALT | MaterialId::CONCRETE)
}

fn is_building_material(mat: MaterialId) -> bool {
    matches!(
        mat,
        MaterialId::CONCRETE | MaterialId::BRICK | MaterialId::GLASS | MaterialId::STEEL
    )
}

fn column_has_material(chunk: &LoadedChunk, vx: i64, vz: i64, target: MaterialId) -> bool {
    let min_v = chunk.id.min_voxel();
    let max_v = chunk.id.max_voxel();
    for vy in min_v.y..max_v.y {
        if chunk.octree.get_voxel(VoxelCoord::new(vx, vy, vz)) == target {
            return true;
        }
    }
    false
}

fn column_has_building_material(
    chunk: &LoadedChunk,
    vx: i64,
    vz: i64,
    ground_y: Option<i64>,
) -> bool {
    let min_v = chunk.id.min_voxel();
    let max_v = chunk.id.max_voxel();
    let start_y = ground_y.map(|y| y + 1).unwrap_or(min_v.y);
    for vy in start_y.max(min_v.y)..max_v.y {
        let mat = chunk.octree.get_voxel(VoxelCoord::new(vx, vy, vz));
        if is_building_material(mat) {
            return true;
        }
    }
    false
}

fn build_chunk_layer_summary(chunk: &LoadedChunk) -> ChunkLayerSummary {
    let min_v = chunk.id.min_voxel();
    let mut columns = Vec::with_capacity((CHUNK_SIZE_X * CHUNK_SIZE_Z) as usize);
    let mut min_ground_y = None;
    let mut max_ground_y = None;
    let mut non_air_columns = 0usize;
    let mut water_columns = 0usize;
    let mut road_columns = 0usize;
    let mut building_columns = 0usize;

    for z in 0..CHUNK_SIZE_Z {
        for x in 0..CHUNK_SIZE_X {
            let vx = min_v.x + x;
            let vz = min_v.z + z;
            let top = top_non_air_in_column(chunk, vx, vz);
            let ground = ground_surface_in_column(chunk, vx, vz);
            let has_water = column_has_material(chunk, vx, vz, MaterialId::WATER);
            let road_surface = top
                .map(|(_, mat)| mat)
                .filter(|mat| is_road_surface(*mat))
                .or_else(|| {
                    ground
                        .map(|(_, mat)| mat)
                        .filter(|mat| is_road_surface(*mat))
                });
            let has_building = column_has_building_material(chunk, vx, vz, ground.map(|(y, _)| y));

            if top.is_some() {
                non_air_columns += 1;
            }
            if has_water {
                water_columns += 1;
            }
            if road_surface.is_some() {
                road_columns += 1;
            }
            if has_building {
                building_columns += 1;
            }
            if let Some((ground_y, _)) = ground {
                min_ground_y =
                    Some(min_ground_y.map_or(ground_y, |min_y: i64| min_y.min(ground_y)));
                max_ground_y =
                    Some(max_ground_y.map_or(ground_y, |max_y: i64| max_y.max(ground_y)));
            }

            columns.push(ColumnLayerInfo {
                top_y: top.map(|(y, _)| y),
                top_material: top.map(|(_, mat)| mat),
                ground_y: ground.map(|(y, _)| y),
                ground_material: ground.map(|(_, mat)| mat),
                local_slope_deg: None,
                local_relief_m: None,
                has_water,
                road_surface,
                has_building,
            });
        }
    }

    for z in 0..CHUNK_SIZE_Z as usize {
        for x in 0..CHUNK_SIZE_X as usize {
            let idx = chunk_layer_index(x, z);
            let Some(center_y) = columns[idx].ground_y else {
                continue;
            };
            let sample = |sx: usize, sz: usize| -> f32 {
                columns[chunk_layer_index(sx, sz)]
                    .ground_y
                    .unwrap_or(center_y) as f32
            };
            let xe = if x + 1 < CHUNK_SIZE_X as usize {
                x + 1
            } else {
                x
            };
            let xw = if x > 0 { x - 1 } else { x };
            let zn = if z + 1 < CHUNK_SIZE_Z as usize {
                z + 1
            } else {
                z
            };
            let zs = if z > 0 { z - 1 } else { z };
            let dx = (xe - xw) as f32;
            let dz = (zn - zs) as f32;
            let dydx = (sample(xe, z) - sample(xw, z)) / dx.max(1.0);
            let dydz = (sample(x, zn) - sample(x, zs)) / dz.max(1.0);
            let grad = (dydx * dydx + dydz * dydz).sqrt();
            let mut neighborhood_min = center_y as f32;
            let mut neighborhood_max = center_y as f32;
            for nz in zs..=zn {
                for nx in xw..=xe {
                    let y = sample(nx, nz);
                    neighborhood_min = neighborhood_min.min(y);
                    neighborhood_max = neighborhood_max.max(y);
                }
            }
            columns[idx].local_slope_deg = Some(grad.atan().to_degrees());
            columns[idx].local_relief_m = Some(neighborhood_max - neighborhood_min);
        }
    }

    ChunkLayerSummary {
        chunk_id: chunk.id,
        last_modified: chunk.last_modified,
        min_voxel_x: min_v.x,
        min_voxel_z: min_v.z,
        min_ground_y,
        max_ground_y,
        non_air_columns,
        water_columns,
        road_columns,
        building_columns,
        columns,
    }
}

fn material_color32(mat: MaterialId) -> egui::Color32 {
    let props = MaterialId::properties(mat);
    egui::Color32::from_rgb(props.color[0], props.color[1], props.color[2])
}

fn layer_cell_color(
    cell: &ColumnLayerInfo,
    summary: &ChunkLayerSummary,
    mode: LayerViewMode,
) -> egui::Color32 {
    match mode {
        LayerViewMode::Off => egui::Color32::TRANSPARENT,
        LayerViewMode::Height => {
            match (cell.ground_y, summary.min_ground_y, summary.max_ground_y) {
                (Some(y), Some(min_y), Some(max_y)) if max_y > min_y => {
                    let t = ((y - min_y) as f32 / (max_y - min_y) as f32).clamp(0.0, 1.0);
                    let shade = (50.0 + t * 205.0) as u8;
                    egui::Color32::from_rgb(shade, shade, shade)
                }
                (Some(_), _, _) => egui::Color32::from_gray(180),
                _ => egui::Color32::from_gray(18),
            }
        }
        LayerViewMode::Slope => match cell.local_slope_deg {
            Some(slope) if slope < 8.0 => {
                let t = (slope / 8.0).clamp(0.0, 1.0);
                egui::Color32::from_rgb(
                    (45.0 + t * 60.0) as u8,
                    (135.0 + t * 60.0) as u8,
                    (55.0 - t * 15.0) as u8,
                )
            }
            Some(slope) if slope < 20.0 => {
                let t = ((slope - 8.0) / 12.0).clamp(0.0, 1.0);
                egui::Color32::from_rgb(
                    (105.0 + t * 150.0) as u8,
                    (195.0 + t * 25.0) as u8,
                    (40.0 - t * 10.0) as u8,
                )
            }
            Some(slope) => {
                let t = ((slope - 20.0) / 30.0).clamp(0.0, 1.0);
                egui::Color32::from_rgb(
                    (255.0 - t * 70.0) as u8,
                    (220.0 - t * 180.0) as u8,
                    (30.0 - t * 10.0).max(0.0) as u8,
                )
            }
            None => egui::Color32::from_gray(18),
        },
        LayerViewMode::Ground => cell
            .ground_material
            .map(material_color32)
            .unwrap_or_else(|| egui::Color32::from_gray(18)),
        LayerViewMode::Hydro => {
            if cell.top_material == Some(MaterialId::WATER) {
                egui::Color32::from_rgb(65, 105, 225)
            } else if cell.has_water {
                egui::Color32::from_rgb(35, 65, 140)
            } else {
                egui::Color32::from_gray(18)
            }
        }
        LayerViewMode::Roads => match cell.road_surface {
            Some(MaterialId::ASPHALT) => egui::Color32::from_gray(90),
            Some(MaterialId::CONCRETE) => egui::Color32::from_gray(180),
            Some(other) => material_color32(other),
            None => egui::Color32::from_gray(18),
        },
        LayerViewMode::Buildings => {
            if cell.has_building {
                egui::Color32::from_rgb(255, 150, 70)
            } else {
                egui::Color32::from_gray(18)
            }
        }
    }
}

fn build_observability_probe(
    chunk_streamer: &ChunkStreamer,
    player_pos: &ECEF,
    player_local: Vec3,
) -> ObservabilityProbe {
    let player_voxel = VoxelCoord::from_ecef(player_pos);
    let player_chunk = ChunkId::from_voxel(&player_voxel);
    let mut probe = ObservabilityProbe {
        player_chunk,
        player_voxel,
        player_local,
        loaded_chunks: chunk_streamer.loaded_chunks().count(),
        chunks_queued: chunk_streamer.stats.chunks_queued,
        chunks_loading: chunk_streamer.stats.chunks_loading,
        loaded_this_frame: chunk_streamer.stats.chunks_loaded_this_frame,
        chunk_state: None,
        chunk_distance_m: None,
        chunk_lod: None,
        chunk_dirty: None,
        chunk_last_modified: None,
        surface_y_f: None,
        top_y: None,
        top_material: None,
        ground_y: None,
        ground_material: None,
        ground_clearance_voxels: None,
        current_material: MaterialId::AIR,
        above_material: MaterialId::AIR,
        below_material: MaterialId::AIR,
    };

    if let Some(chunk) = chunk_streamer.get_chunk(&player_chunk) {
        probe.chunk_state = Some(chunk.state);
        probe.chunk_distance_m = Some(chunk.distance_m);
        probe.chunk_lod = Some(chunk.lod_level);
        probe.chunk_dirty = Some(chunk.dirty);
        probe.chunk_last_modified = Some(chunk.last_modified);
        probe.surface_y_f = chunk
            .surface_cache
            .as_ref()
            .and_then(|cache| cache.get(&(player_voxel.x, player_voxel.z)).copied());
        probe.current_material = chunk.octree.get_voxel(player_voxel);
        probe.above_material = chunk.octree.get_voxel(VoxelCoord::new(
            player_voxel.x,
            player_voxel.y + 1,
            player_voxel.z,
        ));
        probe.below_material = chunk.octree.get_voxel(VoxelCoord::new(
            player_voxel.x,
            player_voxel.y - 1,
            player_voxel.z,
        ));
        if let Some((y, mat)) = top_non_air_in_column(chunk, player_voxel.x, player_voxel.z) {
            probe.top_y = Some(y);
            probe.top_material = Some(mat);
        }
        if let Some((y, mat)) = ground_surface_in_column(chunk, player_voxel.x, player_voxel.z) {
            probe.ground_y = Some(y);
            probe.ground_material = Some(mat);
            probe.ground_clearance_voxels = Some(player_voxel.y - y);
        }
    }

    probe
}

fn sorted_material_counts(counts: HashMap<MaterialId, usize>) -> Vec<(MaterialId, usize)> {
    let mut rows: Vec<_> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| (a.0 as u8).cmp(&(b.0 as u8))));
    rows
}

fn dump_active_chunk_report(chunk: &LoadedChunk, probe: &ObservabilityProbe) {
    let min_v = chunk.id.min_voxel();
    let max_v = chunk.id.max_voxel();
    let mut voxel_counts: HashMap<MaterialId, usize> = HashMap::new();
    let mut top_counts: HashMap<MaterialId, usize> = HashMap::new();
    let mut ground_counts: HashMap<MaterialId, usize> = HashMap::new();
    let mut non_air_columns = 0usize;

    for vx in min_v.x..max_v.x {
        for vz in min_v.z..max_v.z {
            if let Some((_, top_mat)) = top_non_air_in_column(chunk, vx, vz) {
                non_air_columns += 1;
                *top_counts.entry(top_mat).or_insert(0) += 1;
            }
            if let Some((_, ground_mat)) = ground_surface_in_column(chunk, vx, vz) {
                *ground_counts.entry(ground_mat).or_insert(0) += 1;
            }
            for vy in min_v.y..max_v.y {
                let mat = chunk.octree.get_voxel(VoxelCoord::new(vx, vy, vz));
                if mat != MaterialId::AIR {
                    *voxel_counts.entry(mat).or_insert(0) += 1;
                }
            }
        }
    }

    println!(
        "\n🔎 Active chunk dump ({}, {}, {})",
        chunk.id.x, chunk.id.y, chunk.id.z
    );
    println!(
        "   player voxel=({}, {}, {}) local=({:.1}, {:.1}, {:.1})",
        probe.player_voxel.x,
        probe.player_voxel.y,
        probe.player_voxel.z,
        probe.player_local.x,
        probe.player_local.y,
        probe.player_local.z,
    );
    println!(
        "   state={:?} dirty={} lod={} dist={:.1}m last_modified={}",
        chunk.state, chunk.dirty, chunk.lod_level, chunk.distance_m, chunk.last_modified,
    );
    println!(
        "   columns with content: {}/{}",
        non_air_columns,
        ((max_v.x - min_v.x) * (max_v.z - min_v.z)) as usize,
    );

    println!("   top-of-column materials:");
    for (mat, count) in sorted_material_counts(top_counts) {
        println!("     {:>12?}: {}", mat, count);
    }

    println!("   ground-surface materials:");
    for (mat, count) in sorted_material_counts(ground_counts) {
        println!("     {:>12?}: {}", mat, count);
    }

    println!("   voxel histogram:");
    for (mat, count) in sorted_material_counts(voxel_counts) {
        println!("     {:>12?}: {}", mat, count);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum CaptureViewMode {
    Beauty,
    Height,
    Slope,
    Ground,
    Hydro,
    Roads,
    Buildings,
}

impl CaptureViewMode {
    fn label(self) -> &'static str {
        match self {
            Self::Beauty => "beauty",
            Self::Height => "height",
            Self::Slope => "slope",
            Self::Ground => "ground",
            Self::Hydro => "hydro",
            Self::Roads => "roads",
            Self::Buildings => "buildings",
        }
    }

    fn layer_view_mode(self) -> LayerViewMode {
        match self {
            Self::Beauty => LayerViewMode::Off,
            Self::Height => LayerViewMode::Height,
            Self::Slope => LayerViewMode::Slope,
            Self::Ground => LayerViewMode::Ground,
            Self::Hydro => LayerViewMode::Hydro,
            Self::Roads => LayerViewMode::Roads,
            Self::Buildings => LayerViewMode::Buildings,
        }
    }
}

fn default_capture_ground_offset_m() -> f32 {
    6.0
}
fn default_capture_settle_frames() -> u32 {
    15
}
fn default_capture_staging_altitude_m() -> f64 {
    80.0
}
fn default_capture_min_loaded_chunks() -> usize {
    12
}
fn default_capture_loading_timeout_secs() -> u64 {
    90
}
fn default_capture_pitch_deg() -> f32 {
    -20.0
}
fn default_capture_views() -> Vec<CaptureViewMode> {
    vec![CaptureViewMode::Beauty]
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CapturePointConfig {
    name: String,
    lat: f64,
    lon: f64,
    #[serde(default)]
    alt_m: Option<f64>,
    #[serde(default)]
    yaw_deg: f32,
    #[serde(default = "default_capture_pitch_deg")]
    pitch_deg: f32,
    #[serde(default)]
    ground_offset_m: Option<f32>,
    #[serde(default)]
    prefer_water: bool,
    #[serde(default)]
    views: Vec<CaptureViewMode>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CaptureRouteConfig {
    name: String,
    #[serde(default = "default_capture_ground_offset_m")]
    ground_offset_m: f32,
    #[serde(default = "default_capture_settle_frames")]
    settle_frames: u32,
    #[serde(default = "default_capture_staging_altitude_m")]
    staging_altitude_m: f64,
    #[serde(default = "default_capture_min_loaded_chunks")]
    min_loaded_chunks: usize,
    #[serde(default = "default_capture_loading_timeout_secs")]
    loading_timeout_secs: u64,
    #[serde(default = "default_capture_views")]
    views: Vec<CaptureViewMode>,
    points: Vec<CapturePointConfig>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CaptureShotRecord {
    file: String,
    point_name: String,
    view: String,
    target_gps: [f64; 2],
    player_gps: [f64; 3],
    player_orthometric_alt_m: f64,
    anchor_offset_m: f64,
    player_local: [f32; 3],
    player_chunk: Option<[i64; 3]>,
    ground_y: Option<i64>,
    top_material: Option<String>,
    ground_material: Option<String>,
    chunk_state: Option<String>,
    loaded_chunks: Option<usize>,
    queued_chunks: Option<usize>,
    loading_chunks: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CaptureRunMetadata {
    route_name: String,
    route_source: String,
    world_dir: String,
    region: Option<String>,
    started_at_unix_ms: u64,
    status: String,
    failure: Option<String>,
    route: CaptureRouteConfig,
    captures: Vec<CaptureShotRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureStage {
    TeleportPoint,
    WaitForLoading,
    Settling,
    CaptureRequested,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Copy)]
struct CaptureAnchorCandidate {
    voxel: VoxelCoord,
    has_water: bool,
}

struct FrameCaptureRequest {
    output_path: PathBuf,
    log_label: String,
    record: Option<CaptureShotRecord>,
}

struct CaptureRunner {
    route: CaptureRouteConfig,
    route_source: String,
    output_dir: PathBuf,
    metadata_path: PathBuf,
    world_dir: String,
    region: Option<String>,
    started_at_unix_ms: u64,
    stage: CaptureStage,
    point_index: usize,
    view_index: usize,
    settle_frames_remaining: u32,
    load_timed_out: bool,
    load_timeout_started_at: Option<Instant>,
    captures: Vec<CaptureShotRecord>,
    failure: Option<String>,
}

impl CaptureRunner {
    fn from_file(
        path: &Path,
        output_dir: PathBuf,
        world_dir: &Path,
        region: Option<&str>,
    ) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read capture route {}: {}", path.display(), e))?;
        let mut route: CaptureRouteConfig = serde_json::from_str(&raw)
            .map_err(|e| format!("failed to parse capture route {}: {}", path.display(), e))?;
        if route.points.is_empty() {
            return Err(format!("capture route {} has no points", path.display()));
        }
        if route.views.is_empty() {
            route.views = default_capture_views();
        }
        std::fs::create_dir_all(&output_dir).map_err(|e| {
            format!(
                "failed to create capture output dir {}: {}",
                output_dir.display(),
                e
            )
        })?;
        let runner = Self {
            route,
            route_source: path.display().to_string(),
            metadata_path: output_dir.join("metadata.json"),
            output_dir,
            world_dir: world_dir.display().to_string(),
            region: region.map(|s| s.to_string()),
            started_at_unix_ms: unix_timestamp_millis(),
            stage: CaptureStage::TeleportPoint,
            point_index: 0,
            view_index: 0,
            settle_frames_remaining: 0,
            load_timed_out: false,
            load_timeout_started_at: None,
            captures: Vec::new(),
            failure: None,
        };
        runner.write_metadata()?;
        Ok(runner)
    }

    fn current_point(&self) -> Option<&CapturePointConfig> {
        self.route.points.get(self.point_index)
    }

    fn expected_capture_count(&self) -> usize {
        self.route
            .points
            .iter()
            .map(|point| {
                if point.views.is_empty() {
                    self.route.views.len()
                } else {
                    point.views.len()
                }
            })
            .sum()
    }

    fn current_views(&self) -> &[CaptureViewMode] {
        self.current_point()
            .and_then(|point| {
                if point.views.is_empty() {
                    None
                } else {
                    Some(point.views.as_slice())
                }
            })
            .unwrap_or(self.route.views.as_slice())
    }

    fn current_view(&self) -> CaptureViewMode {
        self.current_views()
            .get(self.view_index)
            .copied()
            .unwrap_or(CaptureViewMode::Beauty)
    }

    fn current_ground_offset_m(&self) -> f32 {
        self.current_point()
            .and_then(|point| point.ground_offset_m)
            .unwrap_or(self.route.ground_offset_m)
    }

    fn current_target_alt_m(&self, origin_gps: GPS) -> Option<f64> {
        self.current_point().map(|point| {
            point
                .alt_m
                .unwrap_or(origin_gps.alt + self.route.staging_altitude_m)
        })
    }

    fn current_target_column_voxel(&self) -> Option<VoxelCoord> {
        self.current_point().map(|point| {
            let target_ecef = GPS::new(point.lat, point.lon, 0.0).to_ecef();
            let target_voxel = VoxelCoord::from_ecef(&target_ecef);
            VoxelCoord::new(target_voxel.x, 0, target_voxel.z)
        })
    }

    fn is_running(&self) -> bool {
        matches!(
            self.stage,
            CaptureStage::TeleportPoint
                | CaptureStage::WaitForLoading
                | CaptureStage::Settling
                | CaptureStage::CaptureRequested
        )
    }

    fn should_hide_basic_hud(&self) -> bool {
        self.is_running()
    }

    fn status(&self) -> &'static str {
        match self.stage {
            CaptureStage::Complete => "complete",
            CaptureStage::Failed => "failed",
            _ => "running",
        }
    }

    fn fail(&mut self, error: String) {
        self.failure = Some(error.clone());
        self.stage = CaptureStage::Failed;
        eprintln!("📸 Capture failed: {}", error);
        let _ = self.write_metadata();
    }

    fn write_metadata(&self) -> Result<(), String> {
        let metadata = CaptureRunMetadata {
            route_name: self.route.name.clone(),
            route_source: self.route_source.clone(),
            world_dir: self.world_dir.clone(),
            region: self.region.clone(),
            started_at_unix_ms: self.started_at_unix_ms,
            status: self.status().to_string(),
            failure: self.failure.clone(),
            route: self.route.clone(),
            captures: self.captures.clone(),
        };
        let json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| format!("failed to serialize capture metadata: {}", e))?;
        std::fs::write(&self.metadata_path, json).map_err(|e| {
            format!(
                "failed to write capture metadata {}: {}",
                self.metadata_path.display(),
                e
            )
        })
    }

    fn build_capture_request(
        &self,
        player: &Player,
        physics: &PhysicsWorld,
        origin_gps: GPS,
        origin_voxel: VoxelCoord,
        probe: Option<&ObservabilityProbe>,
    ) -> Option<FrameCaptureRequest> {
        let point = self.current_point()?;
        let view = self.current_view();
        let point_slug = sanitize_capture_name(&point.name);
        let output_path = self.output_dir.join(format!(
            "{:02}_{}__{}.png",
            self.point_index + 1,
            point_slug,
            view.label(),
        ));
        let player_local = physics.ecef_to_local(&player.position);
        let player_gps = open_world_local_to_gps(player_local, origin_gps, origin_voxel);
        let record = CaptureShotRecord {
            file: output_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| output_path.display().to_string()),
            point_name: point.name.clone(),
            view: view.label().to_string(),
            target_gps: [point.lat, point.lon],
            player_gps: [player_gps.lat, player_gps.lon, player_gps.alt],
            player_orthometric_alt_m: gps_orthometric_alt_m(player_gps),
            anchor_offset_m: worldgen_river::haversine_m(
                point.lat,
                point.lon,
                player_gps.lat,
                player_gps.lon,
            ),
            player_local: [player_local.x, player_local.y, player_local.z],
            player_chunk: probe.map(|p| [p.player_chunk.x, p.player_chunk.y, p.player_chunk.z]),
            ground_y: probe.and_then(|p| p.ground_y),
            top_material: probe
                .and_then(|p| p.top_material)
                .map(|mat| format!("{:?}", mat)),
            ground_material: probe
                .and_then(|p| p.ground_material)
                .map(|mat| format!("{:?}", mat)),
            chunk_state: probe
                .and_then(|p| p.chunk_state)
                .map(|state| format!("{:?}", state)),
            loaded_chunks: probe.map(|p| p.loaded_chunks),
            queued_chunks: probe.map(|p| p.chunks_queued),
            loading_chunks: probe.map(|p| p.chunks_loading),
        };
        Some(FrameCaptureRequest {
            output_path,
            log_label: format!(
                "{} [{}/{}] {}",
                point.name,
                self.point_index + 1,
                self.route.points.len(),
                view.label(),
            ),
            record: Some(record),
        })
    }

    fn record_capture(&mut self, record: Option<CaptureShotRecord>) -> Result<(), String> {
        if let Some(record) = record {
            self.captures.push(record);
        }
        const BETWEEN_VIEW_SETTLE_FRAMES: u32 = 2;
        if self.view_index + 1 < self.current_views().len() {
            self.view_index += 1;
            self.stage = CaptureStage::Settling;
            self.settle_frames_remaining = BETWEEN_VIEW_SETTLE_FRAMES;
        } else if self.point_index + 1 < self.route.points.len() {
            self.point_index += 1;
            self.view_index = 0;
            self.stage = CaptureStage::TeleportPoint;
            self.settle_frames_remaining = 0;
            self.load_timed_out = false;
            self.load_timeout_started_at = None;
        } else {
            self.stage = CaptureStage::Complete;
        }
        self.write_metadata()
    }
}

const OPEN_WORLD_WGS84_A: f64 = 6_378_137.0;
const OPEN_WORLD_WGS84_B: f64 = 6_356_752.314_245;

fn open_world_column_to_gps(vx: f64, vz: f64, origin_voxel: VoxelCoord) -> (f64, f64) {
    let ecef_x = (vx + 0.5) + metaverse_core::voxel::WORLD_MIN_METERS;
    let ecef_z = (vz + 0.5) + metaverse_core::voxel::WORLD_MIN_METERS;
    let origin_ecef_y = origin_voxel.to_ecef().y;
    let y_sq =
        OPEN_WORLD_WGS84_A * OPEN_WORLD_WGS84_A * (1.0 - (ecef_z / OPEN_WORLD_WGS84_B).powi(2))
            - ecef_x * ecef_x;
    let ecef_y = if y_sq > 0.0 {
        y_sq.sqrt() * origin_ecef_y.signum()
    } else {
        origin_ecef_y
    };
    let gps = ECEF::new(ecef_x, ecef_y, ecef_z).to_gps();
    (gps.lat, gps.lon)
}

fn open_world_local_to_gps(local: Vec3, origin_gps: GPS, origin_voxel: VoxelCoord) -> GPS {
    let vx = origin_voxel.x as f64 + local.x as f64;
    let vz = origin_voxel.z as f64 + local.z as f64;
    let (lat, lon) = open_world_column_to_gps(vx, vz, origin_voxel);
    GPS::new(lat, lon, origin_gps.alt + local.y as f64)
}

fn gps_orthometric_alt_m(gps: GPS) -> f64 {
    gps.alt - metaverse_core::elevation::egm96_undulation(gps.lat, gps.lon)
}

fn open_world_position_to_gps(
    physics: &PhysicsWorld,
    position: &ECEF,
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
) -> GPS {
    open_world_local_to_gps(physics.ecef_to_local(position), origin_gps, origin_voxel)
}

fn bearing_between_gps_deg(from: GPS, to: GPS) -> f32 {
    let lat1 = from.lat.to_radians();
    let lat2 = to.lat.to_radians();
    let dlon = (to.lon - from.lon).to_radians();
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    ((y.atan2(x).to_degrees() + 360.0) % 360.0) as f32
}

fn open_world_forward_bearing_deg(
    local_pos: Vec3,
    forward: Vec3,
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
) -> Option<f32> {
    let horizontal = Vec3::new(forward.x, 0.0, forward.z);
    if horizontal.length_squared() < 1e-6 {
        return None;
    }
    let step = horizontal.normalize() * 25.0;
    let from = open_world_local_to_gps(local_pos, origin_gps, origin_voxel);
    let to = open_world_local_to_gps(local_pos + step, origin_gps, origin_voxel);
    Some(bearing_between_gps_deg(from, to))
}

fn unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sanitize_capture_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for ch in name.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else {
            if !last_dash && !out.is_empty() {
                out.push('-');
            }
            last_dash = true;
            continue;
        };
        out.push(mapped);
    }
    out.trim_matches('-').to_string()
}

fn capture_anchor_y_at_column(chunk_streamer: &ChunkStreamer, vx: i64, vz: i64) -> Option<i64> {
    chunk_streamer
        .loaded_chunks()
        .filter_map(|chunk| {
            let min_v = chunk.id.min_voxel();
            let max_v = chunk.id.max_voxel();
            if vx < min_v.x || vx >= max_v.x || vz < min_v.z || vz >= max_v.z {
                return None;
            }
            ground_surface_in_column(chunk, vx, vz)
                .or_else(|| top_non_air_in_column(chunk, vx, vz))
                .map(|(y, _)| y)
        })
        .max()
}

fn ensure_capture_target_chunks_loaded(
    chunk_streamer: &mut ChunkStreamer,
    pending_mesh_queue: &mut Vec<ChunkId>,
    tile_store: &metaverse_core::tile_store::TileStore,
    player_chunk: ChunkId,
    target_column_voxel: VoxelCoord,
) {
    let target_chunk_x = target_column_voxel.x.div_euclid(CHUNK_SIZE_X);
    let target_chunk_z = target_column_voxel.z.div_euclid(CHUNK_SIZE_Z);

    // Capture staging altitudes can place the player in the air chunk above the
    // actual terrain surface. Preload the target column's lower slices from the
    // baked store when possible so anchor lookup can see the ground immediately;
    // fall back to async priority requests if a slice is missing from the store.
    for dy in -2..=1 {
        let chunk_id = ChunkId::new(target_chunk_x, player_chunk.y + dy, target_chunk_z);
        if chunk_streamer.is_chunk_loaded(&chunk_id) {
            continue;
        }

        let stored = [
            metaverse_core::tile_store::PassId::Roads,
            metaverse_core::tile_store::PassId::Hydro,
            metaverse_core::tile_store::PassId::Terrain,
        ]
        .into_iter()
        .find_map(|pass| {
            tile_store.get_chunk_pass(
                chunk_id.x as i32,
                chunk_id.y as i32,
                chunk_id.z as i32,
                pass,
            )
        });

        let mut preloaded = false;
        if let Some(data) = stored {
            if data.len() >= 4 {
                let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                if version == metaverse_core::chunk_loader::TERRAIN_CACHE_VERSION {
                    if let Ok(octree) = metaverse_core::voxel::Octree::from_bytes(&data[4..]) {
                        chunk_streamer.preload_chunk(chunk_id, octree, None);
                        pending_mesh_queue.retain(|id| *id != chunk_id);
                        pending_mesh_queue.push(chunk_id);
                        preloaded = true;
                    }
                }
            }
        }

        if !preloaded {
            chunk_streamer.queue_priority(chunk_id);
        }
    }
}

fn capture_anchor_voxel(
    chunk_streamer: &ChunkStreamer,
    point: &CapturePointConfig,
    origin_voxel: VoxelCoord,
) -> Option<(VoxelCoord, f64)> {
    let mut columns: HashMap<(i64, i64), CaptureAnchorCandidate> = HashMap::new();

    for chunk in chunk_streamer.loaded_chunks() {
        let min_v = chunk.id.min_voxel();
        let max_v = chunk.id.max_voxel();
        for vx in min_v.x..max_v.x {
            for vz in min_v.z..max_v.z {
                let top = top_non_air_in_column(chunk, vx, vz);
                let ground = ground_surface_in_column(chunk, vx, vz);
                let water_surface_y = top
                    .filter(|(_, mat)| *mat == MaterialId::WATER)
                    .map(|(y, _)| y);
                let anchor_y = if point.prefer_water {
                    water_surface_y.or_else(|| ground.or(top).map(|(y, _)| y))
                } else {
                    ground.or(top).map(|(y, _)| y)
                };
                let Some(anchor_y) = anchor_y else {
                    continue;
                };
                let candidate = CaptureAnchorCandidate {
                    voxel: VoxelCoord::new(vx, anchor_y, vz),
                    has_water: water_surface_y.is_some()
                        || column_has_material(chunk, vx, vz, MaterialId::WATER),
                };
                columns
                    .entry((vx, vz))
                    .and_modify(|existing| {
                        if candidate.voxel.y > existing.voxel.y {
                            *existing = candidate;
                        }
                    })
                    .or_insert(candidate);
            }
        }
    }

    let mut best_any: Option<(f64, VoxelCoord)> = None;
    let mut best_water: Option<(f64, VoxelCoord)> = None;
    for candidate in columns.values() {
        let (lat, lon) = open_world_column_to_gps(
            candidate.voxel.x as f64,
            candidate.voxel.z as f64,
            origin_voxel,
        );
        let dist_m = worldgen_river::haversine_m(point.lat, point.lon, lat, lon);
        let better_than = |best: &(f64, VoxelCoord)| {
            dist_m + 1e-6 < best.0
                || ((dist_m - best.0).abs() <= 1e-6
                    && (candidate.voxel.x, candidate.voxel.z, candidate.voxel.y)
                        < (best.1.x, best.1.z, best.1.y))
        };

        if best_any.as_ref().is_none_or(|best| better_than(best)) {
            best_any = Some((dist_m, candidate.voxel));
        }
        if candidate.has_water && best_water.as_ref().is_none_or(|best| better_than(best)) {
            best_water = Some((dist_m, candidate.voxel));
        }
    }

    if point.prefer_water {
        if let Some((dist_m, voxel)) = best_water {
            return Some((voxel, dist_m));
        }
    }

    best_any.map(|(dist_m, voxel)| (voxel, dist_m))
}

fn teleport_player_to_local(
    player: &mut Player,
    physics: &mut PhysicsWorld,
    local: Vec3,
    yaw_rad: f32,
    pitch_rad: f32,
) {
    player.position = physics.local_to_ecef(local);
    player.velocity = Vec3::ZERO;
    player.on_ground = false;
    player.camera_yaw = yaw_rad;
    player.camera_pitch = pitch_rad.clamp(-1.5, 1.5);
    if let Some(body) = physics.bodies.get_mut(player.body_handle) {
        body.set_translation(vector![local.x, local.y, local.z], true);
        body.set_linvel(vector![0.0, 0.0, 0.0], true);
    }
}

fn capture_runner_pre_update(
    runner: &mut CaptureRunner,
    player: &mut Player,
    player_mode: &mut PlayerModeLocal,
    physics: &mut PhysicsWorld,
    chunk_streamer: &mut ChunkStreamer,
    tile_store: &metaverse_core::tile_store::TileStore,
    pending_mesh_queue: &mut Vec<ChunkId>,
    player_chunk: &mut ChunkId,
    game_loading: &mut bool,
    loading_start: &mut Instant,
    loading_last_log: &mut Instant,
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
) {
    if runner.stage != CaptureStage::TeleportPoint {
        return;
    }
    let Some(point) = runner.current_point().cloned() else {
        runner.fail("capture route ran out of points".to_string());
        return;
    };
    let Some(target_alt_m) = runner.current_target_alt_m(origin_gps) else {
        runner.fail("capture route could not resolve target altitude".to_string());
        return;
    };
    let Some(target_column_voxel) = runner.current_target_column_voxel() else {
        runner.fail("capture route could not resolve target column".to_string());
        return;
    };
    *player_mode = PlayerModeLocal::Fly;
    let staged_local = Vec3::new(
        (target_column_voxel.x - origin_voxel.x) as f32,
        (target_alt_m - origin_gps.alt) as f32,
        (target_column_voxel.z - origin_voxel.z) as f32,
    );
    teleport_player_to_local(
        player,
        physics,
        staged_local,
        point.yaw_deg.to_radians(),
        point.pitch_deg.to_radians(),
    );
    *player_chunk = ChunkId::from_ecef(&player.position);
    pending_mesh_queue.clear();
    chunk_streamer.update(player.position);
    chunk_streamer.queue_priority(*player_chunk);
    runner.load_timed_out = false;
    runner.load_timeout_started_at = None;
    ensure_capture_target_chunks_loaded(
        chunk_streamer,
        pending_mesh_queue,
        tile_store,
        *player_chunk,
        target_column_voxel,
    );
    *game_loading = true;
    *loading_start = Instant::now();
    *loading_last_log = Instant::now();
    runner.stage = CaptureStage::WaitForLoading;
    println!(
        "📸 Capture route {} — moving to point {}/{}: {}",
        runner.route.name,
        runner.point_index + 1,
        runner.route.points.len(),
        point.name,
    );
}

fn capture_runner_post_update(
    runner: &mut CaptureRunner,
    player: &mut Player,
    physics: &mut PhysicsWorld,
    chunk_streamer: &ChunkStreamer,
    player_chunk: &mut ChunkId,
    game_loading: bool,
    _origin_gps: GPS,
    origin_voxel: VoxelCoord,
    layer_view_mode: &mut LayerViewMode,
) {
    match runner.stage {
        CaptureStage::WaitForLoading => {
            if game_loading {
                return;
            }
            let loaded = chunk_streamer.stats.chunks_loaded;
            let generating = chunk_streamer.stats.chunks_loading;
            let queued = chunk_streamer.stats.chunks_queued;
            let standard_ready =
                loaded >= runner.route.min_loaded_chunks && generating == 0 && queued == 0;
            if !standard_ready && !runner.load_timed_out {
                return;
            }
            let Some(point) = runner.current_point().cloned() else {
                runner.fail("capture route lost current point after loading".to_string());
                return;
            };
            let Some((anchor_voxel, anchor_distance_m)) =
                capture_anchor_voxel(chunk_streamer, &point, origin_voxel)
            else {
                if runner.load_timed_out
                    && runner
                        .load_timeout_started_at
                        .is_some_and(|started| started.elapsed().as_secs() < 10)
                {
                    return;
                }
                runner.fail(format!(
                    "no terrain/water column found for capture point '{}'",
                    point.name
                ));
                return;
            };
            if runner.load_timed_out {
                println!(
                    "📸 Capture point '{}' proceeding after load timeout with {} loaded, {} generating, {} queued",
                    point.name, loaded, generating, queued
                );
            }
            let snapped_local = Vec3::new(
                (anchor_voxel.x - origin_voxel.x) as f32,
                (anchor_voxel.y - origin_voxel.y) as f32 + runner.current_ground_offset_m(),
                (anchor_voxel.z - origin_voxel.z) as f32,
            );
            if anchor_distance_m > 0.5 {
                println!(
                    "📸 Capture point '{}' nudged {:.1}m to nearest populated column",
                    point.name, anchor_distance_m,
                );
            }
            teleport_player_to_local(
                player,
                physics,
                snapped_local,
                point.yaw_deg.to_radians(),
                point.pitch_deg.to_radians(),
            );
            *player_chunk = ChunkId::from_ecef(&player.position);
            *layer_view_mode = runner.current_view().layer_view_mode();
            runner.stage = CaptureStage::Settling;
            runner.settle_frames_remaining = runner.route.settle_frames;
        }
        CaptureStage::Settling => {
            *layer_view_mode = runner.current_view().layer_view_mode();
            if runner.settle_frames_remaining > 0 {
                runner.settle_frames_remaining -= 1;
            } else {
                runner.stage = CaptureStage::CaptureRequested;
            }
        }
        CaptureStage::CaptureRequested => {
            *layer_view_mode = runner.current_view().layer_view_mode();
        }
        _ => {}
    }
}

fn capture_surface_texture(
    context: &RenderContext,
    texture: &wgpu::Texture,
    output_path: &Path,
) -> Result<(), String> {
    let width = context.config.width;
    let height = context.config.height;
    if width == 0 || height == 0 {
        return Err("cannot capture a zero-sized surface".to_string());
    }

    let bytes_per_pixel = 4u32;
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let output_buffer_size = padded_bytes_per_row as u64 * height as u64;
    let output_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("frame_capture_buffer"),
        size: output_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame_capture_encoder"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &output_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    context.queue.submit(std::iter::once(encoder.finish()));

    let buffer_slice = output_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result.map_err(|e| format!("{:?}", e)));
    });
    let _ = context.device.poll(wgpu::Maintain::Wait);
    match rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(format!("failed to map capture buffer: {}", err)),
        Err(err) => return Err(format!("capture map channel failed: {}", err)),
    }

    let mapped = buffer_slice.get_mapped_range();
    let mut rgba = vec![0u8; (width * height * bytes_per_pixel) as usize];
    for (row_index, src_row) in mapped.chunks(padded_bytes_per_row as usize).enumerate() {
        let dst_start = row_index * unpadded_bytes_per_row as usize;
        let dst_end = dst_start + unpadded_bytes_per_row as usize;
        rgba[dst_start..dst_end].copy_from_slice(&src_row[..unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    output_buffer.unmap();

    match context.config.format {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {}
        other => {
            return Err(format!("unsupported screenshot surface format {:?}", other));
        }
    }

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "failed to create screenshot dir {}: {}",
                parent.display(),
                e
            )
        })?;
    }
    image::save_buffer(output_path, &rgba, width, height, image::ColorType::Rgba8)
        .map_err(|e| format!("failed to save screenshot {}: {}", output_path.display(), e))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlayerModeLocal {
    Walk, // Physics-based, can walk/jump
    Fly,  // Free movement, no gravity
}

fn terrain_lod_for_distance(distance_m: f64) -> u8 {
    if distance_m < 200.0 {
        0
    } else if distance_m < 400.0 {
        1
    } else if distance_m < 700.0 {
        2
    } else {
        3
    }
}

fn lod_to_step(lod: u8) -> usize {
    match lod {
        2 => 2,
        3 => 4,
        _ => 1,
    }
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn value_for_flag(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find_map(|pair| (pair[0] == flag).then(|| pair[1].clone()))
}

fn main() {
    env_logger::init();

    // ── Auto-update check (before window opens so exec-restart is clean) ──────
    {
        let rt = tokio::runtime::Runtime::new().expect("tokio rt");
        let result = rt.block_on(async {
            let timeout = std::time::Duration::from_secs(8);
            tokio::time::timeout(
                timeout,
                metaverse_core::autoupdate::check_for_update(
                    "PaddyOhFurnature/mverse",
                    env!("CARGO_PKG_VERSION"),
                ),
            )
            .await
        });
        if let Ok(Some((tag, url, _notes))) = result {
            eprintln!("🔄 Update available: {} — downloading…", tag);
            let apply = rt.block_on(metaverse_core::autoupdate::apply_update(&tag, &url));
            if let Err(e) = apply {
                eprintln!(
                    "⚠️  Auto-update failed: {} — continuing with current version",
                    e
                );
            }
        }
    }

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
    let cli_args: Vec<String> = std::env::args().collect();
    let is_temp = has_flag(&cli_args, "--temp-identity");
    let skip_construct = has_flag(&cli_args, "--noconstruct");
    // --server http://192.168.1.x:8080  — server base URL for world object API
    let server_url: String = value_for_flag(&cli_args, "--server")
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
    // --world-dir <path>  — override world_data directory (e.g. for test scenes)
    let world_dir_override = value_for_flag(&cli_args, "--world-dir");
    // --region <name>  — override the active region (e.g. gympie, brisbane-cbd)
    let region_override = value_for_flag(&cli_args, "--region");
    let capture_route_file = value_for_flag(&cli_args, "--capture-route-file");
    let capture_output_dir = value_for_flag(&cli_args, "--capture-output-dir");
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
        Identity::load_or_create().expect("Failed to create identity")
    };

    println!("   PeerId: {}", short_peer_id(&identity.peer_id()));
    if !needs_signup {
        println!("   Key: ~/.metaverse/identity.key");
    }

    println!("\n🌐 Starting P2P network node...");

    // Clone identity for multiplayer (we need it later for player persistence)
    let mut multiplayer = MultiplayerSystem::new_with_runtime(identity.clone())
        .expect("Failed to create multiplayer system");

    // Start listening on all available transports for maximum connectivity
    // TCP (primary transport) + QUIC (UDP-based, better NAT traversal)
    multiplayer
        .listen_on("/ip4/0.0.0.0/tcp/0")
        .expect("Failed to listen on TCP");
    multiplayer
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1")
        .expect("Failed to listen on QUIC");

    println!("📡 Multi-transport enabled: TCP + QUIC (universal connectivity)");

    // Connect to relay server for NAT traversal
    // Relay running on Android phone: 49.182.84.9:4001
    // Peer ID: 12D3KooWEzai1nEViFuX6JmLWDLU61db7T1A3hyd4xpmGs4W59ge
    let relay_addr =
        "/ip4/49.182.84.9/tcp/4001/p2p/12D3KooWEzai1nEViFuX6JmLWDLU61db7T1A3hyd4xpmGs4W59ge";
    println!("📡 Connecting to relay on phone: {}", relay_addr);
    if let Err(e) = multiplayer.dial(relay_addr) {
        println!(
            "⚠️  Failed to connect to relay: {} (continuing without relay)",
            e
        );
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
            let window = Arc::new(
                el.create_window(
                    winit::window::WindowAttributes::default()
                        .with_title("Phase 1 Multiplayer - Metaverse Core")
                        .with_inner_size(winit::dpi::LogicalSize::new(960, 540)),
                )
                .unwrap(),
            );

            // Initialize renderer
            println!("🎨 Initializing renderer...");
            let mut context = pollster::block_on(RenderContext::new(window.clone()));
            let mut pipeline = RenderPipeline::new(&context);
            // OSM pipeline — flat vertex-colour shader for roads, buildings, water surface.
            // Treats the "normal" vertex slot as RGB so colours pass through without lighting.
            let _osm_pipeline = OsmPipeline::new(
                &context,
                &pipeline.camera_bind_group_layout,
                &pipeline.model_bind_group_layout,
            );
            let water_pipeline = WaterPipeline::new(
                &context,
                &pipeline.camera_bind_group_layout,
                &pipeline.model_bind_group_layout,
            );

            // Textured GLB pipeline — renders inferred world objects (benches, lamps, etc.)
            let textured_pipeline = TexturedPipeline::new(
                &context,
                &pipeline.camera_bind_group_layout,
                &pipeline.model_bind_group_layout,
            );
            // Load one GlbModel per object type from assets/models/*.glb.
            // Missing files are skipped gracefully — objects just won't render.
            let mut object_models: HashMap<String, GlbModel> = HashMap::new();
            for model_name in &[
                "streetlight",
                "traffic_light",
                "bench",
                "bin",
                "letterbox",
                "buoy",
                "gate_post",
            ] {
                let path = format!("assets/models/{model_name}.glb");
                if let Some(m) = textured_pipeline.load_glb(&context.device, &context.queue, &path)
                {
                    object_models.insert(model_name.to_string(), m);
                }
            }
            println!("🏙️  Loaded {} world-object models", object_models.len());

            // Per-placed-object GPU state: (id, model_name, buf, bind_group)
            struct InferredGpu {
                id: String,
                model_name: String,
                _buf: wgpu::Buffer,
                bind_group: wgpu::BindGroup,
            }
            let mut inferred_objects: Vec<InferredGpu> = Vec::new();

            // Road surface texture — now unused (roads are voxel materials, not mesh overlays).
            // Kept here to avoid breaking the texture asset pipeline; remove if asset removed.
            let _road_texture_rgba: Option<(Vec<u8>, u32, u32)> = {
                let tex_path = "assets/textures/road_asphalt.png";
                image::open(tex_path)
                    .ok()
                    .map(|img| img.to_rgba8())
                    .map(|rgba| {
                        let (w, h) = (rgba.width(), rgba.height());
                        (rgba.into_raw(), w, h)
                    })
            };
            if _road_texture_rgba.is_some() {
                println!("🛣️  Road texture loaded (reserved for future textured voxels)");
            }

            // Billboard pipeline — renders textured quads on Construct module room walls
            let billboard_pipeline = BillboardPipeline::new(&context);
            let mut module_billboards: [Option<ModuleBillboards>; 6] = Default::default();
            let mut billboard_frame_counter = 0u32;
            // Placed world-object billboards: (object_id, built billboard). Rebuilt on cache change.
            let mut placed_billboards: Vec<(String, ModuleBillboards)> = Vec::new();

            // WORLDNET terminal screen — rendered onto the kiosk top face
            let terminal_screen =
                TerminalScreen::new(&context, &billboard_pipeline, SIGNUP_TERMINAL_POS);
            let mut worldnet_buf = metaverse_core::worldnet::WorldnetPixelBuffer::new();
            // Render initial page (home, no key yet — will update each frame when near terminal)
            {
                use metaverse_core::worldnet::{WorldnetAddress, render_page};
                render_page(&WorldnetAddress::Root, None, &[], None, &mut worldnet_buf);
                terminal_screen.update(&context.queue, &worldnet_buf);
            }

            // Always-on debug HUD (top-left overlay)
            let mut hud = DebugHud::new(&context, &window);
            let mut observability_mode = ObservabilityMode::Basic;
            let mut layer_view_mode = LayerViewMode::Off;
            let mut active_chunk_layer_summary: Option<ChunkLayerSummary> = None;

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
            // Pass --noconstruct to skip straight to Open World for testing.
            let mut game_mode = if skip_construct {
                GameMode::OpenWorld
            } else {
                GameMode::Construct
            };

            // Setup terrain generation with SRTM data
            println!("🗺️  Setting up chunk-based terrain generation...");

            // Origin: geographic centre of the active region.
            // Both worldgen and client use region.center() so chunk IDs always align.
            // When the region selector is added (WorldOS), this will come from the
            // selected region rather than the hardcoded default.
            let active_region = if let Some(ref name) = region_override {
                metaverse_core::worldgen::RegionBounds::named(name)
                    .or_else(|| {
                        // Try parsing as lat_min,lon_min,lat_max,lon_max
                        let p: Vec<f64> = name.split(',').filter_map(|s| s.parse().ok()).collect();
                        if p.len() == 4 {
                            Some(metaverse_core::worldgen::RegionBounds::new(
                                p[0], p[2], p[1], p[3],
                            ))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        eprintln!("⚠️  Unknown region '{}', falling back to brisbane", name);
                        metaverse_core::worldgen::RegionBounds::named("brisbane").unwrap()
                    })
            } else {
                metaverse_core::worldgen::RegionBounds::named("brisbane")
                    .expect("Default region 'brisbane' not found")
            };
            let (base_lat, base_lon) = active_region.center();

            // Standardise on P2P + OpenTopography — P2P first to avoid unnecessary API calls.
            let data_dir = if let Some(ref d) = world_dir_override {
                std::path::PathBuf::from(d)
            } else {
                std::path::PathBuf::from("world_data")
            };
            // Elevation cache is global shared data — always under the default world_data dir,
            // not under any per-world --world-dir override.
            let elev_cache = std::path::PathBuf::from("world_data").join("elevation_cache");
            let api_key = std::env::var("OPENTOPOGRAPHY_API_KEY").ok();
            let tile_fetcher = Arc::new(multiplayer.tile_fetcher());

            // Open TileStore ONCE — shared by all OSM and SRTM consumers in this process.
            // RocksDB allows only one open per path per process; passing Arc avoids LOCK conflicts.
            let client_tile_store: Arc<metaverse_core::tile_store::TileStore> = Arc::new(
                metaverse_core::tile_store::TileStore::open(&data_dir.join("tiles.db"))
                    .expect("Failed to open client TileStore"),
            );
            // OSM cache — wraps the same TileStore, no second DB open.
            let osm_cache = OsmDiskCache::from_arc(Arc::clone(&client_tile_store));

            let mut elevation_pipeline = ElevationPipeline::new();
            // P2P first — try peers before hitting any API
            elevation_pipeline.add_source(Box::new(P2PElevationSource::new(
                Arc::clone(&tile_fetcher),
                elev_cache.clone(),
            )));
            if let Some(ref key) = api_key {
                elevation_pipeline.add_source(Box::new(OpenTopographySource::new(
                    key.clone(),
                    elev_cache.clone(),
                )));
            } else {
                println!("⚠️  No OPENTOPOGRAPHY_API_KEY set — using free elevation sources only");
            }
            // Free fallback sources — no API key required
            elevation_pipeline.add_source(Box::new(CopernicusElevationSource::with_tile_store(
                elev_cache.clone(),
                Arc::clone(&client_tile_store),
            )));
            elevation_pipeline.add_source(Box::new(SkadiElevationSource::with_tile_store(
                elev_cache.clone(),
                Arc::clone(&client_tile_store),
            )));

            // Prefer origin_gps from manifest.json (written by worldgen) — guarantees the
            // client uses the EXACT same origin that worldgen used when building tiles.db.
            // Without this, a stale SRTM cache hit or fallback-to-zero shifts every chunk.
            let manifest_origin: Option<GPS> = {
                let mp = data_dir.join("manifest.json");
                if mp.exists() {
                    std::fs::read_to_string(&mp)
                        .ok()
                        .and_then(|s| {
                            serde_json::from_str::<metaverse_core::worldgen::RegionManifest>(&s)
                                .ok()
                        })
                        .map(|m| GPS::new(m.origin_gps[0], m.origin_gps[1], m.origin_gps[2]))
                } else {
                    None
                }
            };

            // Query ground elevation at origin (orthometric, EGM96) then convert to WGS-84 ellipsoidal.
            // This ensures terrain delta_y = 0 at the spawn point → player spawns at terrain surface.
            let srtm_origin = elevation_pipeline
                .query_with_fill(&GPS::new(base_lat, base_lon, 0.0))
                .map(|e| e.meters)
                .unwrap_or(0.0); // sea-level fallback if SRTM not yet cached
            let n_origin = metaverse_core::elevation::egm96_undulation(base_lat, base_lon);
            let computed_origin = GPS::new(base_lat, base_lon, srtm_origin + n_origin);
            let origin_gps = manifest_origin.unwrap_or(computed_origin); // WGS-84 ellipsoidal

            if data_dir.join("manifest.json").exists() {
                println!(
                    "   Origin GPS: ({:.6}, {:.6}, {:.3}m) [from manifest]",
                    origin_gps.lat, origin_gps.lon, origin_gps.alt
                );
            } else {
                println!(
                    "   Origin GPS: ({:.6}, {:.6}, {:.1}m ell / {:.1}m ortho) [computed]",
                    origin_gps.lat, origin_gps.lon, origin_gps.alt, srtm_origin
                );
            }

            // Convert GPS origin to voxel coordinates
            let origin_ecef = origin_gps.to_ecef();
            let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
            println!("   Origin voxel: {:?}", origin_voxel);

            // Create terrain generator with origin for coordinate conversion
            let elevation_pipeline_1 = elevation_pipeline;
            let generator = TerrainGenerator::new(elevation_pipeline_1, origin_gps, origin_voxel)
                .without_vegetation();
            let generator_arc = Arc::new(Mutex::new(generator));

            // Create second elevation pipeline for chunk_manager (same source as above)
            let mut elevation_pipeline_2 = ElevationPipeline::new();
            elevation_pipeline_2.add_source(Box::new(P2PElevationSource::new(
                Arc::clone(&tile_fetcher),
                elev_cache.clone(),
            )));
            if let Some(ref key) = api_key {
                elevation_pipeline_2.add_source(Box::new(OpenTopographySource::new(
                    key.clone(),
                    elev_cache.clone(),
                )));
            }
            elevation_pipeline_2.add_source(Box::new(CopernicusElevationSource::with_tile_store(
                elev_cache.clone(),
                Arc::clone(&client_tile_store),
            )));
            elevation_pipeline_2.add_source(Box::new(SkadiElevationSource::with_tile_store(
                elev_cache.clone(),
                Arc::clone(&client_tile_store),
            )));
            let chunk_manager_generator =
                TerrainGenerator::new(elevation_pipeline_2, origin_gps, origin_voxel)
                    .without_vegetation();

            // Trigger cleanup of old flat-file elevation cache tiles in the background
            metaverse_core::tile_store::cleanup_old_tile_dir(&elev_cache);
            metaverse_core::tile_store::cleanup_old_srtm_dir(&elev_cache);

            // User content layer - separates edits from base terrain
            let user_content = Arc::new(Mutex::new(UserContentLayer::new()));
            // Derive at-rest encryption key from identity signing key
            {
                let enc_key =
                    UserContentLayer::derive_encryption_key(&identity.signing_key().to_bytes());
                user_content.lock().unwrap().set_encryption_key(enc_key);
            }
            // Advertise this client's capabilities to the DHT (0 = no storage contribution by default)
            multiplayer.publish_node_capabilities(0);
            // Announce any OSM tiles already cached locally — other peers can find and fetch from us
            multiplayer.announce_cached_osm_tiles(&data_dir.join("osm"));
            // Announce any elevation tiles already cached locally
            multiplayer.announce_cached_elevation_tiles(&elev_cache);

            // World data directory — use --world-dir override if provided, else default
            let world_dir = data_dir.clone();

            // Create world directory if it doesn't exist
            if !world_dir.exists() {
                std::fs::create_dir_all(&world_dir).expect("Failed to create world data directory");
                println!("📁 Created world data directory: {:?}", world_dir);
            }

            // Open RocksDB world store (migrates any existing flat-file ops on first open)
            let world_store_arc = metaverse_core::world_store::WorldStore::open(
                &world_dir.join("world.db"),
                &world_dir,
            )
            .ok()
            .map(std::sync::Arc::new);
            if let Some(ref ws) = world_store_arc {
                user_content
                    .lock()
                    .unwrap()
                    .set_world_store(std::sync::Arc::clone(ws));
                println!("💾 WorldStore opened: {} ops", ws.op_count());
            } else {
                eprintln!("⚠️  WorldStore unavailable — falling back to flat-file ops");
            }

            // Load persisted voxel ops into user_content.
            // When WorldStore is set, load_chunk reads from RocksDB; otherwise falls back to flat files.
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
                                    chunk_ids_to_load.push(metaverse_core::chunk::ChunkId {
                                        x,
                                        y,
                                        z,
                                    });
                                }
                            }
                        }
                    }
                    if !chunk_ids_to_load.is_empty() {
                        match uc.load_chunks(&world_dir, &chunk_ids_to_load) {
                            Ok(counts) => {
                                let total: usize = counts.values().sum();
                                if total > 0 {
                                    println!(
                                        "📂 Loaded {} persisted voxel ops from {} chunks",
                                        total,
                                        counts.len()
                                    );
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
                    println!(
                        "🗄️  Advertising {} local chunks to DHT",
                        startup_chunk_ids.len()
                    );
                    multiplayer.advertise_chunks(&startup_chunk_ids);
                }
            }
            println!("🔄 Initializing chunk streaming system...");
            let stream_config = ChunkStreamerConfig {
                load_radius_m: 150.0,   // 150m view distance
                unload_radius_m: 250.0, // 100m hysteresis — no churn when walking
                max_loaded_chunks: 600, // Headroom above ~300 max at 150m
                safe_zone_radius: 2,    // Keep 5×5 chunks around player (always loaded)
                frame_budget_ms: 5.0,
                max_in_flight: 16, // Only 16 chunks dispatched to workers at once
                fast_travel_threshold_m: 500.0,
            };
            let mut chunk_streamer = ChunkStreamer::new(
                stream_config,
                generator_arc.clone(),
                user_content.clone(),
                world_dir.clone(),
            );

            // Terrain chunks queued only when entering Open World — not needed in Construct.
            // chunk_streamer.update(spawn_ecef);  // deferred until portal transition

            // Keep chunk manager for user edits and voxel operations tracking only
            let chunk_manager_user_content = user_content.lock().unwrap().clone();
            let mut chunk_manager =
                ChunkManager::new(chunk_manager_generator, chunk_manager_user_content);

            // Initialize physics world (empty — terrain colliders added as chunks build in-loop)
            let origin_voxel_ecef = origin_voxel.to_ecef();
            let mut physics = PhysicsWorld::with_origin(origin_voxel_ecef);

            // ── Build the Construct scene ──────────────────────────────────────────────
            // The Construct is always available — floor, pillars, terminals, portal.
            // It loads from bundled geometry with no network or terrain dependency.
            println!("🏛️  Building construct scene...");
            let construct = ConstructScene::build();
            let construct_floor_buffer = MeshBuffer::from_mesh(&context.device, &construct.floor);
            let construct_pillars_buffer =
                MeshBuffer::from_mesh(&context.device, &construct.pillars);
            let construct_terminal_buffer =
                MeshBuffer::from_mesh(&context.device, &construct.signup_terminal);
            let construct_portal_buffer =
                MeshBuffer::from_mesh(&context.device, &construct.world_portal);
            let construct_doors_buffer =
                MeshBuffer::from_mesh(&context.device, &construct.module_doors);

            // Add the construct floor as a static physics collider so the player
            // has ground to stand on from frame 1 — no terrain streaming needed.
            let floor_collision = metaverse_core::construct::build_floor_collision_mesh();
            metaverse_core::physics::create_collision_from_mesh(
                &mut physics,
                &floor_collision,
                &origin_voxel,
                None,
            );
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

            // Fetch content for all sections from the server so billboards are populated
            // immediately on first render rather than waiting for gossipsub messages.
            // The channel carries batches of ContentItems from the background thread.
            let (content_inbox_tx, content_inbox_rx) =
                std::sync::mpsc::channel::<Vec<metaverse_core::meshsite::ContentItem>>();
            {
                use metaverse_core::meshsite::{ContentItem, Section};
                let url = server_url.clone();
                let tx = content_inbox_tx.clone();
                println!("📰 Fetching content from server {}…", url);
                std::thread::spawn(move || {
                    let client = match reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(8))
                        .build()
                    {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    for section in ["forums", "wiki", "marketplace", "post"] {
                        let endpoint =
                            format!("{}/api/v1/content?section={}&limit=50", url, section);
                        let Ok(resp) = client.get(&endpoint).send() else {
                            continue;
                        };
                        let Ok(json) = resp.json::<Vec<serde_json::Value>>() else {
                            continue;
                        };
                        let items: Vec<ContentItem> = json
                            .into_iter()
                            .filter_map(|v| {
                                Some(ContentItem {
                                    id: v["id"].as_str()?.to_string(),
                                    section: Section::from_str(
                                        v["section"].as_str().unwrap_or("forums"),
                                    )
                                    .unwrap_or(Section::Forums),
                                    title: v["title"].as_str().unwrap_or("").to_string(),
                                    body: v["body"].as_str().unwrap_or("").to_string(),
                                    author: v["author"].as_str().unwrap_or("").to_string(),
                                    signature: vec![],
                                    created_at: v["created_at"].as_u64().unwrap_or(0),
                                })
                            })
                            .collect();
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
                (dx * dx + dy * dy + dz * dz).sqrt()
            };
            let use_saved = dist_from_origin < 2000.0 && dist_from_origin > 0.001;
            if !use_saved {
                println!(
                    "   ⚠️  Saved position {:.0}m from spawn — resetting to spawn",
                    dist_from_origin
                );
            }

            let initial_position = if use_saved {
                player_state.position
            } else {
                // Offset 2m above terrain surface (radially outward in ECEF = "up" at any lat/lon)
                // so feet clear the surface voxel and terrain collision can load before walking.
                let mag = (spawn_ecef_origin.x * spawn_ecef_origin.x
                    + spawn_ecef_origin.y * spawn_ecef_origin.y
                    + spawn_ecef_origin.z * spawn_ecef_origin.z)
                    .sqrt();
                ECEF::new(
                    spawn_ecef_origin.x + spawn_ecef_origin.x / mag * 30.0,
                    spawn_ecef_origin.y + spawn_ecef_origin.y / mag * 30.0,
                    spawn_ecef_origin.z + spawn_ecef_origin.z / mag * 30.0,
                )
            };
            let initial_gps = if use_saved {
                open_world_position_to_gps(
                    &physics,
                    &player_state.position,
                    origin_gps,
                    origin_voxel,
                )
            } else {
                origin_gps
            };

            // Create player at saved position (or default if no save)
            let mut player = Player::new(&mut physics, initial_gps, player_state.yaw);
            player.position = initial_position;
            player.camera_yaw = player_state.yaw;
            player.camera_pitch = player_state.pitch;

            // Sync the rapier body to the actual spawn position so Walk mode picks it up correctly.
            // (Player::new places the body based on GPS, but initial_position may differ slightly.)
            {
                let spawn_local = physics.ecef_to_local(&initial_position);
                if let Some(body) = physics.bodies.get_mut(player.body_handle) {
                    body.set_translation(
                        vector![spawn_local.x, spawn_local.y, spawn_local.z],
                        true,
                    );
                }
            }

            // In Construct mode, override position to Construct spawn (its own floor at Y=0).
            // In OpenWorld (--noconstruct or after portal entry), keep the saved position.
            if matches!(game_mode, GameMode::Construct) {
                let spawn_local =
                    metaverse_core::construct::SPAWN_POINT + glam::Vec3::new(0.0, 2.5, 0.0);
                let spawn_ecef = physics.local_to_ecef(spawn_local);
                player.position = spawn_ecef;
                if let Some(body) = physics.bodies.get_mut(player.body_handle) {
                    body.set_translation(
                        vector![spawn_local.x, spawn_local.y, spawn_local.z],
                        true,
                    );
                }
            }

            // Determine which chunk the player is actually standing in and prioritise it
            // so it is dispatched to a worker thread before any surrounding chunks.
            // Only relevant in OpenWorld mode — in Construct we don't need terrain chunks.
            // mut: updated at portal transition to track the current open-world spawn chunk.
            let mut player_chunk = ChunkId::from_ecef(&player.position);

            // ── Queue spawn chunk with priority (OpenWorld only) ─────────────────────
            // Async: the loading phase builds mesh + collider once the worker finishes.
            // (Previously this was synchronous, but that blocks the event loop for seconds.)
            if game_mode == GameMode::OpenWorld {
                chunk_streamer.queue_priority(player_chunk);
                println!("   Player chunk: {} — queued with priority", player_chunk);
            }

            let player_local = physics.ecef_to_local(&player.position);
            println!(
                "✅ Player position at local: ({:.1}, {:.1}, {:.1})",
                player_local.x, player_local.y, player_local.z
            );

            // Camera setup - first person from player eyes
            let camera_local = player.camera_position_local(&physics);
            let mut camera = Camera::new(camera_local, 1920.0 / 1080.0);
            camera.yaw = player.camera_yaw;
            camera.pitch = player.camera_pitch;

            // Model transform bind groups
            let player_model_matrix = Mat4::from_rotation_translation(
                glam::Quat::from_rotation_y(player.camera_yaw),
                player_local,
            );
            let (player_model_uniform, player_model_bind_group) =
                pipeline.create_model_bind_group(&context.device, &player_model_matrix);

            let crosshair_matrix = Mat4::IDENTITY;
            let (crosshair_uniform, crosshair_bind_group) =
                pipeline.create_model_bind_group(&context.device, &crosshair_matrix);

            // Remote player bind groups (create one per remote player as needed)
            let mut remote_player_bind_groups: HashMap<
                libp2p::PeerId,
                (wgpu::Buffer, wgpu::BindGroup),
            > = HashMap::new();

            // Input state
            let mut input_forward = 0.0f32;
            let mut input_right = 0.0f32;
            let mut input_up = 0.0f32;
            let mut jump_pressed = false;
            let mut dig_pressed = false;
            let mut place_pressed = false;
            let mut chat_pressed = false;
            // In open-world testing (--noconstruct) start in Fly so the player doesn't
            // fall underground before terrain collision meshes are built.
            // Press F to toggle to Walk mode once oriented.
            let mut player_mode = if skip_construct {
                PlayerModeLocal::Fly
            } else {
                PlayerModeLocal::Walk
            };

            let mut _last_frame = Instant::now();
            let mut frame_count = 0;
            let mut fps_timer = Instant::now();
            let mut last_stats_print = Instant::now();
            let mut last_state_resync = Instant::now();
            let mut last_periodic_save = Instant::now();
            let render_start = Instant::now();
            // DHT fallback: query providers for loaded chunks if gossipsub sync hasn't
            // delivered ops after 10s in OpenWorld mode with no peers.
            let mut dht_fallback_at: Option<Instant> = None;
            let mut dht_fallback_done = false;

            // HUD data — updated every physics frame, read by render
            let mut hud_near_terminal: bool = false;
            let mut hud_near_module: Option<usize> = None;

            // Current WORLDNET address shown on the terminal screen
            let mut terminal_address = metaverse_core::worldnet::WorldnetAddress::Root;
            let mut terminal_active = false; // true = keyboard routes to terminal
            let mut terminal_input = String::new(); // current command being typed

            let mut cursor_grabbed = false;

            // Track local voxel operations for CRDT merge
            let mut local_voxel_ops: HashMap<
                VoxelCoord,
                metaverse_core::messages::SignedOperation,
            > = HashMap::new();

            // Loading phase: true until enough spawn-area chunks have meshes and collision built.
            // The event loop renders the loading bar while this is true.
            // In Construct mode we skip terrain loading entirely — floor is ready from frame 1.
            const LOADING_TARGET: usize = 30;
            let mut game_loading = game_mode != GameMode::Construct;
            let mut _loading_frames: u32 = 0;
            let mut loading_start = Instant::now(); // wall-clock timeout (independent of frame rate)
            let mut loading_last_log = Instant::now(); // wall-clock for progress messages
            // Rate-limited mesh-build queue: chunks finish loading faster than we can build
            // meshes in one frame (15+ chunks × mesh+physics ≈ seconds on a single frame).
            // We drain newly_loaded_chunks into this buffer and process at most N per frame.
            let mut pending_mesh_queue: Vec<metaverse_core::chunk::ChunkId> = Vec::new();
            let mut queued_frame_capture: Option<FrameCaptureRequest> = None;
            let mut capture_runner = if let Some(route_file) = capture_route_file.as_deref() {
                let route_path = PathBuf::from(route_file);
                let default_output_dir = PathBuf::from("screenshot").join(format!(
                    "{}-{}",
                    route_path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .map(sanitize_capture_name)
                        .filter(|stem| !stem.is_empty())
                        .unwrap_or_else(|| "capture".to_string()),
                    unix_timestamp_millis(),
                ));
                let output_dir = capture_output_dir
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or(default_output_dir);
                let runner = CaptureRunner::from_file(
                    &route_path,
                    output_dir,
                    &world_dir,
                    region_override.as_deref(),
                )
                .expect("failed to initialize capture route");
                println!(
                    "📸 Loaded capture route '{}' from {}",
                    runner.route.name, runner.route_source,
                );
                println!("   Output: {}", runner.output_dir.display());
                println!(
                    "   Capture load timeout: {}s | min loaded chunks: {}",
                    runner.route.loading_timeout_secs, runner.route.min_loaded_chunks,
                );
                println!("   Expected captures: {}", runner.expected_capture_count());
                Some(runner)
            } else {
                None
            };

            println!("\n🌍 Loading spawn area (chunks stream in during first frames)...");
            println!(
                "   Target: {} chunks, spawn chunk must have collider",
                LOADING_TARGET
            );
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
                                    println!(
                                        "   ✅ Saved {} operations across {} chunks",
                                        total,
                                        saved.len()
                                    );
                                }
                                Err(e) => {
                                    eprintln!("   ⚠️  Failed to save chunks: {}", e);
                                }
                            }

                            // Save player position
                            let mut player_state = PlayerPersistence::from_state(
                                player.position,
                                player.camera_yaw,
                                player.camera_pitch,
                                if player_mode == PlayerModeLocal::Walk {
                                    MovementMode::Walk
                                } else {
                                    MovementMode::Fly
                                },
                            );
                            player_state.gps = open_world_position_to_gps(
                                &physics,
                                &player.position,
                                origin_gps,
                                origin_voxel,
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
                                let movement_mode_byte = if player_mode == PlayerModeLocal::Walk {
                                    0u8
                                } else {
                                    1u8
                                };
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
                            if signup.is_some() {
                                return;
                            }
                            if placement.is_some() {
                                if event.state == ElementState::Pressed {
                                    if let PhysicalKey::Code(KeyCode::Escape) = event.physical_key {
                                        placement = None;
                                        window.set_cursor_visible(false);
                                        let _ = window
                                            .set_cursor_grab(winit::window::CursorGrabMode::Locked);
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
                                    TerminalCmd, addr_section, process_terminal_command,
                                    render_page, render_terminal_prompt,
                                };
                                // Helper: re-render page + prompt and upload to screen
                                macro_rules! refresh {
                                    () => {{
                                        let key_type = identity
                                            .load_key_record()
                                            .map(|kr| kr.effective_key_type());
                                        let content = multiplayer
                                            .get_content(addr_section(&terminal_address));
                                        let wctx = {
                                            use metaverse_core::worldnet::{
                                                ObjectSummary, OverrideSummary, WorldQueryContext,
                                            };
                                            let mut ctx = WorldQueryContext::new();
                                            for obj in multiplayer.all_world_objects() {
                                                ctx.add_object(ObjectSummary {
                                                    id: obj.id.clone(),
                                                    label: obj.label.clone(),
                                                    type_name: format!("{:?}", obj.object_type),
                                                    position: obj.position,
                                                    chunk: obj.chunk_coords(),
                                                });
                                            }
                                            for (chunk, overrides) in
                                                multiplayer.all_object_overrides()
                                            {
                                                for ov in overrides {
                                                    ctx.add_override(
                                                        chunk,
                                                        OverrideSummary {
                                                            action: format!("{:?}", ov.action),
                                                            timestamp: ov.timestamp,
                                                        },
                                                    );
                                                }
                                            }
                                            ctx
                                        };
                                        render_page(
                                            &terminal_address,
                                            key_type,
                                            content,
                                            Some(&wctx),
                                            &mut worldnet_buf,
                                        );
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
                                        let key_type = identity
                                            .load_key_record()
                                            .map(|kr| kr.effective_key_type());
                                        let content = multiplayer
                                            .get_content(addr_section(&terminal_address));
                                        render_page(
                                            &terminal_address,
                                            key_type,
                                            content,
                                            None,
                                            &mut worldnet_buf,
                                        );
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
                                                    &context,
                                                    &window,
                                                    metaverse_core::meshsite::Section::Forums,
                                                    author,
                                                ));
                                                window.set_cursor_visible(true);
                                                let _ = window.set_cursor_grab(
                                                    winit::window::CursorGrabMode::None,
                                                );
                                            }
                                            TerminalCmd::Close => {
                                                terminal_active = false;
                                                let key_type = identity
                                                    .load_key_record()
                                                    .map(|kr| kr.effective_key_type());
                                                let content = multiplayer
                                                    .get_content(addr_section(&terminal_address));
                                                render_page(
                                                    &terminal_address,
                                                    key_type,
                                                    content,
                                                    None,
                                                    &mut worldnet_buf,
                                                );
                                                terminal_screen
                                                    .update(&context.queue, &worldnet_buf);
                                                let _ = window.set_cursor_grab(
                                                    winit::window::CursorGrabMode::Locked,
                                                );
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
                                            let _ = window.set_cursor_grab(
                                                winit::window::CursorGrabMode::None,
                                            );
                                            cursor_grabbed = false;
                                            println!("🖱️  Mouse released");
                                        }
                                        KeyCode::F12 => {
                                            if capture_runner
                                                .as_ref()
                                                .map(|runner| runner.is_running())
                                                .unwrap_or(false)
                                            {
                                                println!(
                                                    "📸 Capture route is already driving screenshots"
                                                );
                                            } else if queued_frame_capture.is_some() {
                                                println!(
                                                    "📸 Screenshot already queued for this frame"
                                                );
                                            } else {
                                                let output_path = PathBuf::from("screenshot")
                                                    .join("manual")
                                                    .join(format!(
                                                        "manual-{}.png",
                                                        unix_timestamp_millis()
                                                    ));
                                                queued_frame_capture = Some(FrameCaptureRequest {
                                                    output_path: output_path.clone(),
                                                    log_label: "manual screenshot".to_string(),
                                                    record: None,
                                                });
                                                println!(
                                                    "📸 Queued screenshot: {}",
                                                    output_path.display()
                                                );
                                            }
                                        }
                                        KeyCode::Backquote => {
                                            observability_mode = observability_mode.next();
                                            println!(
                                                "🔎 Observability HUD: {}",
                                                observability_mode.label()
                                            );
                                        }
                                        KeyCode::F9 => {
                                            layer_view_mode = layer_view_mode.next();
                                            println!("🗺️  Layer view: {}", layer_view_mode.label());
                                        }
                                        KeyCode::F10 => {
                                            if game_mode != GameMode::OpenWorld {
                                                println!(
                                                    "🔎 Chunk dump only applies in Open World mode"
                                                );
                                            } else {
                                                let active_chunk =
                                                    ChunkId::from_ecef(&player.position);
                                                let player_local =
                                                    physics.ecef_to_local(&player.position);
                                                let probe = build_observability_probe(
                                                    &chunk_streamer,
                                                    &player.position,
                                                    player_local,
                                                );
                                                if let Some(chunk) =
                                                    chunk_streamer.get_chunk(&active_chunk)
                                                {
                                                    dump_active_chunk_report(chunk, &probe);
                                                } else {
                                                    println!(
                                                        "🔎 Active chunk ({}, {}, {}) is not loaded yet",
                                                        active_chunk.x,
                                                        active_chunk.y,
                                                        active_chunk.z,
                                                    );
                                                }
                                            }
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
                                                addr_section, render_page, render_terminal_prompt,
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
                                                if let Some(section) = MODULE_SECTIONS[idx].clone()
                                                {
                                                    let author = multiplayer.peer_id().to_string();
                                                    compose = Some(ComposeScreen::new(
                                                        &context, &window, section, author,
                                                    ));
                                                    window.set_cursor_visible(true);
                                                    let _ = window.set_cursor_grab(
                                                        winit::window::CursorGrabMode::None,
                                                    );
                                                }
                                            } else if hud_near_terminal {
                                                // Activate terminal — release cursor, show prompt
                                                terminal_active = true;
                                                terminal_input.clear();
                                                let key_type = identity
                                                    .load_key_record()
                                                    .map(|kr| kr.effective_key_type());
                                                let content = multiplayer
                                                    .get_content(addr_section(&terminal_address));
                                                render_page(
                                                    &terminal_address,
                                                    key_type,
                                                    content,
                                                    None,
                                                    &mut worldnet_buf,
                                                );
                                                render_terminal_prompt(
                                                    &terminal_input,
                                                    &mut worldnet_buf,
                                                );
                                                terminal_screen
                                                    .update(&context.queue, &worldnet_buf);
                                                window.set_cursor_visible(true);
                                                let _ = window.set_cursor_grab(
                                                    winit::window::CursorGrabMode::None,
                                                );
                                                cursor_grabbed = false;
                                                println!(
                                                    "🖥️  WORLDNET terminal active — type commands, ESC to exit"
                                                );
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
                                            let _ = window.set_cursor_grab(
                                                winit::window::CursorGrabMode::None,
                                            );
                                        }
                                        // Q/E no longer dig/place — use mouse buttons
                                        _ => {}
                                    }
                                }
                            } else if event.state == ElementState::Released {
                                if let PhysicalKey::Code(keycode) = event.physical_key {
                                    match keycode {
                                        KeyCode::KeyW | KeyCode::KeyS => input_forward = 0.0,
                                        KeyCode::KeyA | KeyCode::KeyD => input_right = 0.0,
                                        KeyCode::Space
                                        | KeyCode::ShiftLeft
                                        | KeyCode::ShiftRight => input_up = 0.0,
                                        _ => {}
                                    }
                                }
                            }
                        }

                        WindowEvent::MouseInput {
                            button,
                            state: ElementState::Pressed,
                            ..
                        } => {
                            // Don't act while signup screen is visible
                            if signup.is_some() {
                                return;
                            }
                            match button {
                                MouseButton::Left => {
                                    // Grab cursor on first left-click (enter FPS mode), then dig
                                    if !cursor_grabbed {
                                        let _ = window.set_cursor_grab(
                                            winit::window::CursorGrabMode::Confined,
                                        );
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
                            let loading_timeout_secs = capture_runner
                                .as_ref()
                                .map(|runner| runner.route.loading_timeout_secs)
                                .unwrap_or(60);

                            if let Some(runner) = capture_runner.as_mut() {
                                capture_runner_pre_update(
                                    runner,
                                    &mut player,
                                    &mut player_mode,
                                    &mut physics,
                                    &mut chunk_streamer,
                                    client_tile_store.as_ref(),
                                    &mut pending_mesh_queue,
                                    &mut player_chunk,
                                    &mut game_loading,
                                    &mut loading_start,
                                    &mut loading_last_log,
                                    origin_gps,
                                    origin_voxel,
                                );
                            }

                            // ── Loading phase ──────────────────────────────────────────────
                            if game_loading {
                                // Keep streaming chunks and building meshes each frame
                                chunk_streamer.update(player.position);
                                chunk_streamer.process_queues(20.0);

                                // Build mesh + collider for any chunks that finished loading.
                                // Prioritise the player chunk: if it's in the queue, move it to front
                                // so it always gets a collider in the very next frame. This unblocks
                                // the game-start condition without waiting for all 85 chunks.
                                pending_mesh_queue
                                    .extend(chunk_streamer.newly_loaded_chunks.drain(..));
                                if let Some(pos) =
                                    pending_mesh_queue.iter().position(|id| *id == player_chunk)
                                {
                                    pending_mesh_queue.swap(0, pos);
                                }
                                // Rate-limited to 3 mesh builds per frame; collider is expensive
                                // (~300-500ms/chunk) so only build it for the player chunk + nearby
                                // chunks (< 90m). Far chunks get a render mesh only — collider built
                                // lazily in the approach section below when the player gets close.
                                const MESH_PER_FRAME: usize = 3;
                                const INITIAL_COLLIDER_RANGE_M: f32 = 90.0;
                                const INITIAL_COLLIDER_PER_FRAME: usize = 1;
                                let mut initial_colliders_built = 0;
                                let player_local_pos_init = physics.ecef_to_local(&player.position);
                                let batch_end = pending_mesh_queue.len().min(MESH_PER_FRAME);
                                let new_chunks: Vec<_> =
                                    pending_mesh_queue.drain(..batch_end).collect();
                                for chunk_id in &new_chunks {
                                    // Clone neighbour surface caches before the mutable borrow so the
                                    // boundary grid points use the exact values the neighbour computed.
                                    let nx_sc = chunk_streamer
                                        .get_chunk(&ChunkId::new(
                                            chunk_id.x + 1,
                                            chunk_id.y,
                                            chunk_id.z,
                                        ))
                                        .and_then(|c| c.surface_cache.clone());
                                    let nz_sc = chunk_streamer
                                        .get_chunk(&ChunkId::new(
                                            chunk_id.x,
                                            chunk_id.y,
                                            chunk_id.z + 1,
                                        ))
                                        .and_then(|c| c.surface_cache.clone());
                                    let ny_lower_sc = chunk_streamer
                                        .get_chunk(&ChunkId::new(
                                            chunk_id.x,
                                            chunk_id.y - 1,
                                            chunk_id.z,
                                        ))
                                        .and_then(|c| c.surface_cache.clone());
                                    let ny_upper_sc = chunk_streamer
                                        .get_chunk(&ChunkId::new(
                                            chunk_id.x,
                                            chunk_id.y + 1,
                                            chunk_id.z,
                                        ))
                                        .and_then(|c| c.surface_cache.clone());
                                    if let Some(chunk_data) = chunk_streamer.get_chunk_mut(chunk_id)
                                    {
                                        let min_v = chunk_data.id.min_voxel();
                                        let max_v = chunk_data.id.max_voxel();
                                        let (mut mesh, chunk_center) =
                                            match &chunk_data.surface_cache {
                                                Some(sc) => extract_chunk_mesh_smooth(
                                                    &chunk_data.octree,
                                                    sc,
                                                    &min_v,
                                                    &max_v,
                                                    nx_sc.as_ref(),
                                                    nz_sc.as_ref(),
                                                    ny_lower_sc.as_ref(),
                                                    ny_upper_sc.as_ref(),
                                                    1,
                                                ),
                                                None => extract_chunk_mesh(
                                                    &chunk_data.octree,
                                                    &min_v,
                                                    &max_v,
                                                    1,
                                                ),
                                            };
                                        let offset = Vec3::new(
                                            (chunk_center.x - origin_voxel.x) as f32,
                                            (chunk_center.y - origin_voxel.y) as f32,
                                            (chunk_center.z - origin_voxel.z) as f32,
                                        );
                                        if !mesh.vertices.is_empty() {
                                            for v in &mut mesh.vertices {
                                                v.position += offset;
                                            }
                                            chunk_data.mesh_buffer =
                                                Some(MeshBuffer::from_mesh(&context.device, &mesh));
                                            let chunk_dist =
                                                (player_local_pos_init - offset).length();
                                            let is_player_chunk = *chunk_id == player_chunk;
                                            if (is_player_chunk
                                                || chunk_dist < INITIAL_COLLIDER_RANGE_M)
                                                && initial_colliders_built
                                                    < INITIAL_COLLIDER_PER_FRAME
                                            {
                                                let collider = metaverse_core::physics::create_collision_from_mesh(
                                            &mut physics, &mesh, &origin_voxel, None);
                                                chunk_data.collider = Some(collider);
                                                initial_colliders_built += 1;
                                            }
                                        }
                                        // Water surface mesh (flat quads on top of WATER voxels)
                                        let mut water_mesh = extract_water_surface_mesh(
                                            &chunk_data.octree,
                                            &min_v,
                                            &max_v,
                                        );
                                        if !water_mesh.vertices.is_empty() {
                                            for v in &mut water_mesh.vertices {
                                                v.position += offset;
                                            }
                                            chunk_data.water_mesh_buffer = Some(
                                                MeshBuffer::from_mesh(&context.device, &water_mesh),
                                            );
                                        }
                                        chunk_data.dirty = false;
                                    }
                                }

                                _loading_frames += 1;

                                let loaded = chunk_streamer.stats.chunks_loaded;
                                let generating = chunk_streamer.stats.chunks_loading;
                                let queued = chunk_streamer.stats.chunks_queued;
                                let capture_loading_ready = capture_runner
                                    .as_ref()
                                    .filter(|runner| runner.stage == CaptureStage::WaitForLoading)
                                    .map(|runner| {
                                        loaded >= runner.route.min_loaded_chunks
                                            && generating == 0
                                            && queued == 0
                                            && pending_mesh_queue.is_empty()
                                            && runner
                                                .current_point()
                                                .and_then(|point| {
                                                    capture_anchor_voxel(
                                                        &chunk_streamer,
                                                        point,
                                                        origin_voxel,
                                                    )
                                                })
                                                .is_some()
                                    })
                                    .unwrap_or(false);
                                let player_chunk_ready = chunk_streamer
                                    .get_chunk(&player_chunk)
                                    .map(|c| c.collider.is_some())
                                    .unwrap_or(false);

                                // Progress feedback every ~1 second of wall-clock time.
                                // Frame-count based (% 60) breaks at low frame rates during
                                // heavy mesh/collider building (1.5s+ per frame → 90s between logs).
                                if loading_last_log.elapsed().as_secs_f32() >= 1.0 {
                                    loading_last_log = Instant::now();
                                    let player_status = if capture_loading_ready {
                                        "capture-ready"
                                    } else if player_chunk_ready {
                                        "ready"
                                    } else {
                                        "waiting"
                                    };
                                    println!(
                                        "⏳ Loading chunks: {}/{} | generating: {} | queued: {} | mesh pending: {} | player chunk: {}",
                                        loaded,
                                        LOADING_TARGET,
                                        generating,
                                        queued,
                                        pending_mesh_queue.len(),
                                        player_status
                                    );
                                    let _ = window.set_title(&format!(
                                        "Metaverse — Loading {}/{} chunks (player: {})",
                                        loaded, LOADING_TARGET, player_status
                                    ));
                                }

                                // Transition to game only when:
                                //  1. Minimum frames elapsed
                                //  2. The chunk the player is ACTUALLY standing in has a collider
                                //     (prevents falling through terrain on first physics step)
                                //  3. Enough surrounding chunks are also ready (or queue drained)
                                // Player chunk is "ready" when it has data loaded AND a physics collider.
                                // Because we prioritise the player chunk in pending_mesh_queue above,
                                // it will get a collider within 1-2 frames of its data arriving.
                                let enough_chunks = loaded >= LOADING_TARGET;
                                let queue_drained = chunk_streamer.stats.chunks_loading == 0
                                    && chunk_streamer.stats.chunks_queued == 0
                                    && pending_mesh_queue.is_empty()
                                    && loaded > 0;

                                if loading_start.elapsed().as_secs() >= 2
                                    && (capture_loading_ready
                                        || (player_chunk_ready && (enough_chunks || queue_drained)))
                                {
                                    let ready_reason = if capture_loading_ready {
                                        "capture anchor ready"
                                    } else {
                                        "player chunk ready"
                                    };
                                    println!(
                                        "✅ Spawn area loaded ({} chunks), {} — starting game",
                                        loaded, ready_reason
                                    );
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
                                    // Queue all loaded chunks for inference — they were drained into
                                    // pending_mesh_queue during loading and never saw the inference loop.
                                    chunk_streamer
                                        .newly_loaded_chunks
                                        .extend(chunk_streamer.loaded_chunk_ids());
                                } else if loading_start.elapsed().as_secs() >= loading_timeout_secs
                                {
                                    // Wall-clock timeout: start anyway after the configured wait regardless of frame count.
                                    // Collider builds can make individual frames very slow, so frame-count
                                    // timeouts are unreliable.
                                    if let Some(runner) = capture_runner.as_mut().filter(|runner| {
                                        runner.stage == CaptureStage::WaitForLoading
                                    }) {
                                        runner.load_timed_out = true;
                                        if runner.load_timeout_started_at.is_none() {
                                            runner.load_timeout_started_at = Some(Instant::now());
                                        }
                                    }
                                    println!(
                                        "⚠️  Loading timeout after {}s — starting with {} chunks (generating: {}, player chunk: {})",
                                        loading_timeout_secs,
                                        loaded,
                                        generating,
                                        capture_loading_ready || player_chunk_ready
                                    );
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
                                    chunk_streamer
                                        .newly_loaded_chunks
                                        .extend(chunk_streamer.loaded_chunk_ids());
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
                                        10.0,
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
                                    if let Ok(op) =
                                        multiplayer.broadcast_voxel_operation(dug, Material::Air)
                                    {
                                        // Save to user content layer (persistence)
                                        user_content
                                            .lock()
                                            .unwrap()
                                            .add_local_operation(op.clone());

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
                                        10.0,
                                    ) {
                                        // Place on the face that was hit (adjacent to hit voxel)
                                        let place_voxel = VoxelCoord::new(
                                            hit.voxel.x + hit.face_normal.0,
                                            hit.voxel.y + hit.face_normal.1,
                                            hit.voxel.z + hit.face_normal.2,
                                        );

                                        // Check player collision before placing
                                        let place_local =
                                            physics.ecef_to_local(&place_voxel.to_ecef());
                                        let player_local = physics.ecef_to_local(&player.position);
                                        let capsule_radius = 0.4;
                                        let capsule_height = 1.8;

                                        // Check if voxel would overlap with player capsule
                                        // Player position is at feet, capsule extends up
                                        let dx = (place_local.x - player_local.x).abs();
                                        let dy = place_local.y - player_local.y; // Relative Y (positive = above player)
                                        let dz = (place_local.z - player_local.z).abs();

                                        // Horizontal distance check (XZ plane)
                                        let horizontal_dist = (dx * dx + dz * dz).sqrt();

                                        // Only block placement if voxel is:
                                        // - Within capsule radius horizontally AND
                                        // - Between player's feet and head (0 to capsule_height)
                                        let blocks_player = horizontal_dist < capsule_radius
                                            && dy >= 0.0
                                            && dy <= capsule_height;

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
                                    if let Some(place_chunk) =
                                        chunk_streamer.get_chunk_mut(&place_chunk_id)
                                    {
                                        place_chunk
                                            .octree
                                            .set_voxel(place_voxel, MaterialId::STONE);
                                        place_chunk.dirty = true;
                                        chunk_streamer.touch_chunk(&place_chunk_id);

                                        println!("🧱 Placed voxel at {:?}", place_voxel);

                                        // Broadcast voxel operation and save to user content
                                        if let Ok(op) = multiplayer
                                            .broadcast_voxel_operation(place_voxel, Material::Stone)
                                        {
                                            // Save to user content layer (persistence)
                                            user_content
                                                .lock()
                                                .unwrap()
                                                .add_local_operation(op.clone());

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
                                let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                                if dist < 2000.0 {
                                    player.position = dht_ecef;
                                    player.camera_yaw = session.rotation[0];
                                    player.camera_pitch = session.rotation[1];
                                    println!(
                                        "📍 Restored position from DHT session record ({:.0}m from spawn)",
                                        dist
                                    );
                                } else {
                                    println!(
                                        "📍 DHT session record too far ({:.0}m) — keeping spawn",
                                        dist
                                    );
                                }
                            }

                            // Process any received voxel operations
                            let pending_ops = multiplayer.take_pending_operations();
                            if !pending_ops.is_empty() {
                                println!(
                                    "📦 Processing {} received voxel operations",
                                    pending_ops.len()
                                );
                                for op in pending_ops {
                                    // Apply to the appropriate chunk
                                    if let (Some(coord), Some(material)) =
                                        (op.coord(), op.material())
                                    {
                                        let chunk_id = ChunkId::from_voxel(&coord);
                                        if let Some(chunk_data) =
                                            chunk_streamer.get_chunk_mut(&chunk_id)
                                        {
                                            let material_id = material.to_material_id();
                                            chunk_data.octree.set_voxel(coord, material_id);
                                            chunk_data.dirty = true;

                                            // Save to BOTH user_content (for ChunkStreamer persistence) AND chunk_manager (for CRDT)
                                            user_content
                                                .lock()
                                                .unwrap()
                                                .add_local_operation(op.clone());
                                            chunk_manager.add_operation(op.clone());

                                            println!(
                                                "✅ Applied remote voxel operation at {:?}",
                                                coord
                                            );
                                        } else {
                                            // Operation for unloaded chunk - still save it for when chunk loads
                                            user_content
                                                .lock()
                                                .unwrap()
                                                .add_local_operation(op.clone());
                                            chunk_manager.add_operation(op.clone());
                                            println!(
                                                "⚠️  Remote operation for unloaded chunk {} - saved for later",
                                                chunk_id
                                            );
                                        }
                                    }
                                }
                            }

                            // Process any received state synchronization operations
                            let state_ops = multiplayer.take_pending_state_operations();
                            if !state_ops.is_empty() {
                                println!(
                                    "📥 Merging {} historical operations from peers",
                                    state_ops.len()
                                );

                                // Apply to chunk_manager for CRDT
                                let applied =
                                    chunk_manager.merge_received_operations(state_ops.clone());

                                // Also save to user_content for persistence
                                for op in &state_ops {
                                    user_content.lock().unwrap().add_local_operation(op.clone());

                                    // Apply to loaded chunks if they're in memory
                                    if let (Some(coord), Some(material)) =
                                        (op.coord(), op.material())
                                    {
                                        let chunk_id = ChunkId::from_voxel(&coord);
                                        if let Some(chunk_data) =
                                            chunk_streamer.get_chunk_mut(&chunk_id)
                                        {
                                            let material_id = material.to_material_id();
                                            chunk_data.octree.set_voxel(coord, material_id);
                                            chunk_data.dirty = true;
                                        }
                                    }
                                }

                                println!(
                                    "   ✅ Applied {} operations (after deduplication)",
                                    applied
                                );
                            }

                            // Check for newly discovered peers and perform full bidirectional state sync
                            if multiplayer.has_new_peers() {
                                let new_peers = multiplayer.get_new_peers();
                                println!(
                                    "🆕 Detected {} new peers, syncing state...",
                                    new_peers.len()
                                );
                                let loaded_chunk_ids = chunk_streamer.loaded_chunk_ids();

                                // Send our chunk manifest so peer knows what we have and when.
                                // Each side sends manifests; each side sends chunks where theirs is newer.
                                // This prevents mutual overwrite and the terrain cliff feedback loop.
                                let manifest = chunk_streamer.chunk_manifest();
                                println!(
                                    "📋 Broadcasting chunk manifest ({} entries)",
                                    manifest.len()
                                );
                                if let Err(e) = multiplayer.broadcast_chunk_manifest(manifest) {
                                    eprintln!("   ⚠️  Failed to broadcast manifest: {}", e);
                                }

                                // Request their op state (pull)
                                if let Err(e) =
                                    multiplayer.request_chunk_state(loaded_chunk_ids.clone())
                                {
                                    eprintln!("   ⚠️  Failed to request chunk state: {}", e);
                                }

                                // Push our ops proactively so they don't have to wait for request round-trip
                                let our_ops: std::collections::HashMap<_, _> = {
                                    let cl = VectorClock::new(); // empty clock = send all
                                    chunk_manager
                                        .filter_operations_for_chunks(&loaded_chunk_ids, &cl)
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
                                    let loaded_set: std::collections::HashSet<
                                        metaverse_core::chunk::ChunkId,
                                    > = chunk_streamer.loaded_chunk_ids().iter().copied().collect();
                                    let _ = multiplayer.update_subscribed_chunks(&loaded_set);
                                }
                            }
                            if multiplayer.peer_count() > 0
                                && last_state_resync.elapsed().as_secs() >= 60
                            {
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
                                        println!(
                                            "💾 [AutoSave] {} ops across {} chunks",
                                            total,
                                            saved.len()
                                        );
                                        // Re-advertise chunks we just saved
                                        multiplayer.advertise_chunks(
                                            &saved.keys().cloned().collect::<Vec<_>>(),
                                        );
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
                                println!(
                                    "🗄️  [DHT] Got {} provider(s) for {}",
                                    providers.len(),
                                    key_str
                                );
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
                                println!(
                                    "📨 Processing state request from {} for {} chunks",
                                    peer_id,
                                    request.chunk_ids.len()
                                );

                                let filtered_ops = chunk_manager.filter_operations_for_chunks(
                                    &request.chunk_ids,
                                    &request.requester_clock,
                                );

                                if !filtered_ops.is_empty() {
                                    println!(
                                        "   → Sending {} operations across {} chunks",
                                        filtered_ops.values().map(|v| v.len()).sum::<usize>(),
                                        filtered_ops.len()
                                    );
                                    if let Err(e) =
                                        multiplayer.send_chunk_state_response(filtered_ops)
                                    {
                                        eprintln!("   ⚠️  Failed to send state response: {}", e);
                                    }
                                } else {
                                    println!("   → No new operations to send");
                                }
                            }

                            // Process received chunk manifests — send chunks where we are newer
                            let manifests = multiplayer.take_pending_chunk_manifests();
                            for peer_manifest in manifests {
                                let peer_map: std::collections::HashMap<ChunkId, u64> =
                                    peer_manifest.into_iter().collect();
                                let mut sent = 0;
                                for chunk_id in chunk_streamer.loaded_chunk_ids() {
                                    if let Some(chunk) = chunk_streamer.get_chunk(&chunk_id) {
                                        let peer_ts = peer_map.get(&chunk_id).copied().unwrap_or(0);
                                        if chunk.last_modified > peer_ts {
                                            // We have a newer version — send it
                                            match chunk.octree.to_bytes() {
                                                Ok(bytes) => {
                                                    if let Err(e) = multiplayer
                                                        .broadcast_chunk_terrain(
                                                            chunk_id,
                                                            bytes,
                                                            chunk.last_modified,
                                                        )
                                                    {
                                                        eprintln!(
                                                            "   ⚠️  Failed to send terrain for {:?}: {}",
                                                            chunk_id, e
                                                        );
                                                    } else {
                                                        sent += 1;
                                                    }
                                                }
                                                Err(e) => eprintln!(
                                                    "   ⚠️  Failed to serialize chunk {:?}: {}",
                                                    chunk_id, e
                                                ),
                                            }
                                        }
                                    }
                                }
                                if sent > 0 {
                                    println!(
                                        "📦 [TERRAIN SYNC] Sent {} chunks newer than peer",
                                        sent
                                    );
                                } else {
                                    println!(
                                        "📋 [TERRAIN SYNC] Peer has same or newer terrain, no chunks sent"
                                    );
                                }
                            }

                            // Apply received chunk terrain data — only if received timestamp is newer than ours
                            let terrain_updates = multiplayer.take_pending_chunk_terrain();
                            if !terrain_updates.is_empty() {
                                println!(
                                    "🌍 [TERRAIN SYNC] Processing {} chunk terrain updates from peers",
                                    terrain_updates.len()
                                );
                                for (chunk_id, octree_bytes, last_modified) in terrain_updates {
                                    match metaverse_core::voxel::Octree::from_bytes(&octree_bytes) {
                                        Ok(octree) => {
                                            if chunk_streamer.replace_chunk_octree(
                                                &chunk_id,
                                                octree,
                                                last_modified,
                                            ) {
                                                println!(
                                                    "   ✅ Applied newer terrain for chunk {:?} (t={})",
                                                    chunk_id, last_modified
                                                );
                                            } else {
                                                println!(
                                                    "   ⏭️  Chunk {:?} rejected (our version same/newer, or not loaded)",
                                                    chunk_id
                                                );
                                            }
                                        }
                                        Err(e) => eprintln!(
                                            "   ⚠️  Failed to deserialize terrain for {:?}: {}",
                                            chunk_id, e
                                        ),
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
                                let fly_direction = forward * move_input.z
                                    + right * move_input.x
                                    + up * move_input.y;

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
                            let velocity =
                                [player.velocity.x, player.velocity.y, player.velocity.z];

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
                            let dist_portal = (WORLD_PORTAL_POS - ploc3).length();
                            let dist_terminal = (SIGNUP_TERMINAL_POS - ploc3).length();
                            let near_signup = dist_terminal < INTERACT_RADIUS;
                            let near_portal = dist_portal < INTERACT_RADIUS;

                            // Detect nearest module screen wall within interact radius
                            // Player must be INSIDE the room (near the back wall) to interact.
                            hud_near_module = MODULES
                                .iter()
                                .enumerate()
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
                                let key_type =
                                    identity.load_key_record().map(|kr| kr.effective_key_type());
                                let addr = WorldnetAddress::Root;
                                let content = multiplayer.get_content("forums");
                                render_page(&addr, key_type, content, None, &mut worldnet_buf);
                                terminal_screen.update(&context.queue, &worldnet_buf);
                            }

                            // Auto-trigger signup overlay if player walks to terminal
                            // and no identity exists yet.
                            if near_signup && signup.is_none() && !Identity::key_file_exists() {
                                println!("🖥️  [Construct] Player at signup terminal");
                                signup = Some(SignupScreen::new(&context, &window));
                            }

                            // World portal: walk through to enter the open world.
                            if near_portal && game_mode == GameMode::Construct {
                                println!(
                                    "🌐 Walking through world portal — entering Open World..."
                                );
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
                                        vector![
                                            world_spawn_local.x,
                                            world_spawn_local.y,
                                            world_spawn_local.z
                                        ],
                                        true,
                                    );
                                    body.set_linvel(vector![0.0, 0.0, 0.0], true);
                                }

                                // 3. Queue spawn chunk as highest priority — loading phase will build it.
                                let spawn_chunk = ChunkId::from_ecef(&player.position);
                                chunk_streamer.queue_priority(spawn_chunk);
                                // Update player_chunk so the loading-phase collider check tracks the
                                // correct open-world chunk (not the old Construct spawn chunk).
                                player_chunk = spawn_chunk;

                                // 4. Re-enter loading phase so terrain streams in before gameplay.
                                game_loading = true;
                                _loading_frames = 0;
                                loading_start = Instant::now();
                                loading_last_log = Instant::now();
                                // Reset DHT fallback so it re-evaluates after terrain loads
                                dht_fallback_at = None;
                                dht_fallback_done = false;

                                // 5. Kick off surrounding chunk streaming.
                                chunk_streamer.update(player.position);

                                println!(
                                    "🌍 Open World — local ({:.1}, {:.1}, {:.1})",
                                    world_spawn_local.x, world_spawn_local.y, world_spawn_local.z
                                );
                            }

                            jump_pressed = false;

                            // Terrain streaming only runs in Open World mode.
                            if game_mode == GameMode::OpenWorld {
                                const FRAME_BUDGET_MS: f64 = 16.0;
                                chunk_streamer.update(player.position);
                                chunk_streamer.process_queues(FRAME_BUDGET_MS);

                                // Mark chunks dirty when LOD level changes (player moved closer/farther)
                                {
                                    let lod_dirty_ids: Vec<ChunkId> = chunk_streamer
                                        .loaded_chunks()
                                        .filter(|c| {
                                            !c.dirty
                                                && terrain_lod_for_distance(c.distance_m)
                                                    != c.lod_level
                                        })
                                        .map(|c| c.id)
                                        .collect();
                                    for id in lod_dirty_ids {
                                        if let Some(c) = chunk_streamer.get_chunk_mut(&id) {
                                            c.dirty = true;
                                        }
                                    }
                                }

                                // Broadcast newly loaded chunk manifests to connected peers.
                                // This lets peers replace their independently-generated terrain with ours
                                // if they haven't loaded this chunk yet (or ours is newer due to user edits).
                                // Drain new chunks into the pending queue; keep a snapshot for AOI+manifest.
                                let new_this_frame: Vec<_> =
                                    chunk_streamer.newly_loaded_chunks.drain(..).collect();
                                if !new_this_frame.is_empty() {
                                    // AOI subscriptions and manifest broadcast fire for each newly arrived
                                    // chunk (cheap operations, no rate-limiting needed).
                                    if !new_this_frame.is_empty() {
                                        // Always update AOI subscriptions when loaded chunks change —
                                        // do NOT gate on peer_count because we need to be subscribed
                                        // before the first peer connects, not after.
                                        let loaded_set: std::collections::HashSet<
                                            metaverse_core::chunk::ChunkId,
                                        > = chunk_streamer
                                            .loaded_chunk_ids()
                                            .iter()
                                            .copied()
                                            .collect();
                                        let _ = multiplayer.update_subscribed_chunks(&loaded_set);

                                        // Manifest broadcast only makes sense when peers are present
                                        if multiplayer.peer_count() > 0 {
                                            let new_entries: Vec<_> = new_this_frame
                                                .iter()
                                                .filter_map(|id| {
                                                    chunk_streamer
                                                        .get_chunk(id)
                                                        .map(|c| (*id, c.last_modified))
                                                })
                                                .collect();
                                            if !new_entries.is_empty() {
                                                let _ = multiplayer
                                                    .broadcast_chunk_manifest(new_entries);
                                            }
                                        }
                                    }

                                    // ── World-object inference for each newly loaded chunk ──────────
                                    // Skips chunks where OSM data isn't cached locally (returns empty).
                                    // Also skips chunks already inferred this session (dedup).
                                    for chunk_id in &new_this_frame {
                                        let chunk_bounds = chunk_id.gps_bounds();
                                        let (cx, cz) = {
                                            use metaverse_core::world_objects::chunk_coords_for_pos;
                                            let min_v = chunk_id.min_voxel();
                                            let cx_f = (min_v.x - origin_voxel.x) as f32;
                                            let cz_f = (min_v.z - origin_voxel.z) as f32;
                                            chunk_coords_for_pos(cx_f, cz_f)
                                        };

                                        // Always request inference status + overrides from DHT,
                                        // even if already inferred locally (other peers may have data).
                                        multiplayer.request_inference_status(cx, cz);
                                        multiplayer.fetch_object_overrides_for_chunk(cx, cz);

                                        if multiplayer.is_chunk_inferred(cx, cz) {
                                            continue;
                                        }

                                        if let Some(chunk) = chunk_streamer.get_chunk(chunk_id) {
                                            let (lat_min, lat_max, lon_min, lon_max) = chunk_bounds;
                                            let osm = fetch_osm_for_chunk_with_cache(
                                                lat_min, lat_max, lon_min, lon_max, &osm_cache,
                                            );
                                            if osm.is_empty() {
                                                continue;
                                            }

                                            let mut new_objs =
                                                world_inference::infer_objects_for_chunk(
                                                    chunk_id,
                                                    &osm,
                                                    &chunk.octree,
                                                    &origin_voxel,
                                                );

                                            // Apply any overrides already cached from DHT
                                            let overrides =
                                                multiplayer.get_object_overrides(cx, cz).to_vec();
                                            world_inference::apply_overrides(
                                                &mut new_objs,
                                                &overrides,
                                            );

                                            for obj in new_objs {
                                                if inferred_objects
                                                    .iter()
                                                    .any(|g: &InferredGpu| g.id == obj.id)
                                                {
                                                    continue;
                                                }
                                                if let metaverse_core::world_objects::ObjectType::Custom(ref mn) = obj.object_type {
                                        if object_models.contains_key(mn.as_str()) {
                                            let model_matrix = Mat4::from_scale_rotation_translation(
                                                Vec3::splat(obj.scale),
                                                glam::Quat::from_rotation_y(obj.rotation_y),
                                                Vec3::new(obj.position[0], obj.position[1], obj.position[2]));
                                            let (buf, bg) = pipeline.create_model_bind_group(
                                                &context.device, &model_matrix);
                                            inferred_objects.push(InferredGpu {
                                                id: obj.id.clone(),
                                                model_name: mn.clone(),
                                                _buf: buf,
                                                bind_group: bg,
                                            });
                                        }
                                    }
                                                multiplayer.register_inferred_object(obj);
                                            }
                                            multiplayer.mark_chunk_inferred(cx, cz);
                                        }
                                    }

                                    // ── Apply pending override refreshes (from DHT arrivals) ────────
                                    let refreshes: Vec<_> =
                                        multiplayer.pending_override_refreshes.drain(..).collect();
                                    for (rcx, rcz) in refreshes {
                                        let overrides =
                                            multiplayer.get_object_overrides(rcx, rcz).to_vec();
                                        // Rebuild GPU entries for objects in this chunk that have overrides
                                        for ov in &overrides {
                                            if !ov.verify() {
                                                continue;
                                            }
                                            use metaverse_core::world_objects::OverrideAction;
                                            match &ov.action {
                                                OverrideAction::Remove => {
                                                    inferred_objects
                                                        .retain(|g| g.id != ov.target_id);
                                                }
                                                OverrideAction::Move {
                                                    position,
                                                    rotation_y,
                                                } => {
                                                    if let Some(g) = inferred_objects
                                                        .iter_mut()
                                                        .find(|g| g.id == ov.target_id)
                                                    {
                                                        let mat = Mat4::from_rotation_translation(
                                                            glam::Quat::from_rotation_y(
                                                                *rotation_y,
                                                            ),
                                                            Vec3::new(
                                                                position[0],
                                                                position[1],
                                                                position[2],
                                                            ),
                                                        );
                                                        context.queue.write_buffer(
                                                            &g._buf,
                                                            0,
                                                            bytemuck::cast_slice(mat.as_ref()),
                                                        );
                                                    }
                                                }
                                                OverrideAction::Replace { new_type } => {
                                                    if let Some(g) = inferred_objects
                                                        .iter_mut()
                                                        .find(|g| g.id == ov.target_id)
                                                    {
                                                        if object_models
                                                            .contains_key(new_type.as_str())
                                                        {
                                                            g.model_name = new_type.clone();
                                                        }
                                                    }
                                                }
                                                OverrideAction::Scale { scale } => {
                                                    if let Some(g) = inferred_objects
                                                        .iter_mut()
                                                        .find(|g| g.id == ov.target_id)
                                                    {
                                                        // Rebuild with new scale — need original position from world_objects_cache
                                                        if let Some(obj) = multiplayer
                                                            .all_world_objects()
                                                            .find(|o| o.id == ov.target_id)
                                                        {
                                                            let mat = Mat4::from_scale_rotation_translation(
                                                    Vec3::splat(*scale),
                                                    glam::Quat::from_rotation_y(obj.rotation_y),
                                                    Vec3::new(obj.position[0], obj.position[1], obj.position[2]));
                                                            context.queue.write_buffer(
                                                                &g._buf,
                                                                0,
                                                                bytemuck::cast_slice(mat.as_ref()),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                // Debug: Log streaming activity (not every frame, too spammy)
                                if frame_count % 120 == 0 {
                                    let has_activity = chunk_streamer.stats.chunks_queued > 0
                                        || chunk_streamer.stats.chunks_loading > 0
                                        || chunk_streamer.stats.chunks_loaded_this_frame > 0;

                                    if has_activity {
                                        println!(
                                            "🌍 ChunkStreamer: {} loaded, {} queued, {} loading",
                                            chunk_streamer.stats.chunks_loaded,
                                            chunk_streamer.stats.chunks_queued,
                                            chunk_streamer.stats.chunks_loading
                                        );
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
                                camera.position + hitbox_offset,
                            );
                            context.queue.write_buffer(
                                &player_model_uniform,
                                0,
                                bytemuck::cast_slice(player_model_matrix.as_ref()),
                            );

                            // Update crosshair
                            let crosshair_pos = camera.position + player.camera_forward() * 0.5;
                            let crosshair_matrix = Mat4::from_translation(crosshair_pos);
                            context.queue.write_buffer(
                                &crosshair_uniform,
                                0,
                                bytemuck::cast_slice(crosshair_matrix.as_ref()),
                            );

                            // Update remote player transforms
                            let remote_count = multiplayer.remote_players().count();
                            for remote in multiplayer.remote_players() {
                                let transform = remote_player_transform(remote, &physics);
                                let local_pos = physics.ecef_to_local(&remote.position);

                                // Debug: Log remote player rendering every 60 frames
                                if frame_count % 60 == 0 {
                                    println!(
                                        "🎨 Rendering remote player at Local=({:.1}, {:.1}, {:.1})",
                                        local_pos.x, local_pos.y, local_pos.z
                                    );
                                }

                                // Get or create bind group for this peer
                                if !remote_player_bind_groups.contains_key(&remote.peer_id) {
                                    let (uniform, bind_group) = pipeline
                                        .create_model_bind_group(&context.device, &transform);
                                    remote_player_bind_groups
                                        .insert(remote.peer_id, (uniform, bind_group));
                                    println!(
                                        "✨ Created bind group for remote player: {}",
                                        short_peer_id(&remote.peer_id)
                                    );
                                } else {
                                    // Update existing transform
                                    let (uniform, _) =
                                        remote_player_bind_groups.get(&remote.peer_id).unwrap();
                                    context.queue.write_buffer(
                                        uniform,
                                        0,
                                        bytemuck::cast_slice(transform.as_ref()),
                                    );
                                }
                            }

                            if frame_count % 60 == 0 && remote_count > 0 {
                                println!("📊 Remote players to render: {}", remote_count);
                            }

                            // Regenerate dirty chunks (per-chunk, not global), rate-limited to
                            // avoid multi-second frame stalls. The slow part is building the rapier
                            // trimesh BVH collider (~300-500ms each). Strategy:
                            //   - Build render mesh for ALL dirty chunks (fast, ~5ms each) — no rate limit
                            //   - Build collider only for chunks close to the player (< 90m) — rate limited
                            // Far chunks get render mesh but no collider; collider added lazily on approach.
                            // If collider already exists (voxel-op rebuild), always rebuild it.
                            const COLLIDER_RANGE_M: f32 = 90.0; // ~3 chunk widths
                            const COLLIDER_PER_FRAME: usize = 1;
                            const DIRTY_PER_FRAME: usize = 4; // process up to 4 dirty re-meshes/frame
                            let mut colliders_built = 0;
                            let mut _dirty_done = 0;
                            let player_local_pos = physics.ecef_to_local(&player.position);
                            // Collect dirty chunk IDs first so we can do immutable neighbour lookups
                            // without conflicting with the mutable borrow on the same map.
                            let dirty_ids: Vec<ChunkId> = chunk_streamer
                                .loaded_chunks()
                                .filter(|c| c.dirty)
                                .take(DIRTY_PER_FRAME)
                                .map(|c| c.id)
                                .collect();
                            for chunk_id in &dirty_ids {
                                let nx_sc = chunk_streamer
                                    .get_chunk(&ChunkId::new(
                                        chunk_id.x + 1,
                                        chunk_id.y,
                                        chunk_id.z,
                                    ))
                                    .and_then(|c| c.surface_cache.clone());
                                let nz_sc = chunk_streamer
                                    .get_chunk(&ChunkId::new(
                                        chunk_id.x,
                                        chunk_id.y,
                                        chunk_id.z + 1,
                                    ))
                                    .and_then(|c| c.surface_cache.clone());
                                let ny_lower_sc = chunk_streamer
                                    .get_chunk(&ChunkId::new(
                                        chunk_id.x,
                                        chunk_id.y - 1,
                                        chunk_id.z,
                                    ))
                                    .and_then(|c| c.surface_cache.clone());
                                let ny_upper_sc = chunk_streamer
                                    .get_chunk(&ChunkId::new(
                                        chunk_id.x,
                                        chunk_id.y + 1,
                                        chunk_id.z,
                                    ))
                                    .and_then(|c| c.surface_cache.clone());
                                if let Some(chunk_data) = chunk_streamer.get_chunk_mut(chunk_id) {
                                    let min_voxel = chunk_data.id.min_voxel();
                                    let max_voxel = chunk_data.id.max_voxel();
                                    let target_lod_lvl =
                                        terrain_lod_for_distance(chunk_data.distance_m);
                                    let step = lod_to_step(target_lod_lvl);
                                    // Use smooth meshing (Y-boundary stitching) for all LOD levels —
                                    // fast MC clamps chunk-edge positions creating visible seam walls.
                                    // Smooth MC uses the same step as fast MC at LOD 2+ but stitches
                                    // correctly across chunk boundaries.
                                    let (mut new_mesh, chunk_center) =
                                        match &chunk_data.surface_cache {
                                            Some(sc) => extract_chunk_mesh_smooth(
                                                &chunk_data.octree,
                                                sc,
                                                &min_voxel,
                                                &max_voxel,
                                                nx_sc.as_ref(),
                                                nz_sc.as_ref(),
                                                ny_lower_sc.as_ref(),
                                                ny_upper_sc.as_ref(),
                                                step,
                                            ),
                                            None => extract_chunk_mesh(
                                                &chunk_data.octree,
                                                &min_voxel,
                                                &max_voxel,
                                                step,
                                            ),
                                        };

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

                                        chunk_data.mesh_buffer =
                                            Some(MeshBuffer::from_mesh(&context.device, &new_mesh));

                                        // Only build the expensive trimesh collider for nearby chunks or
                                        // when replacing an existing collider (after a voxel op).
                                        let is_rebuild = chunk_data.collider.is_some();
                                        let chunk_dist = (player_local_pos - offset).length();
                                        if (chunk_dist < COLLIDER_RANGE_M || is_rebuild)
                                            && colliders_built < COLLIDER_PER_FRAME
                                        {
                                            let new_collider =
                                                metaverse_core::physics::create_collision_from_mesh(
                                                    &mut physics,
                                                    &new_mesh,
                                                    &origin_voxel,
                                                    chunk_data.collider,
                                                );
                                            chunk_data.collider = Some(new_collider);
                                            colliders_built += 1;
                                        }
                                    } else {
                                        chunk_data.mesh_buffer = None;
                                        // Voxels removed → clear collider too
                                        if let Some(handle) = chunk_data.collider.take() {
                                            physics.colliders.remove(
                                                handle,
                                                &mut physics.islands,
                                                &mut physics.bodies,
                                                false,
                                            );
                                        }
                                    }
                                    // Rebuild water surface mesh
                                    let mut new_water = extract_water_surface_mesh(
                                        &chunk_data.octree,
                                        &min_voxel,
                                        &max_voxel,
                                    );
                                    if !new_water.vertices.is_empty() {
                                        for v in &mut new_water.vertices {
                                            v.position[0] += offset.x;
                                            v.position[1] += offset.y;
                                            v.position[2] += offset.z;
                                        }
                                        chunk_data.water_mesh_buffer = Some(MeshBuffer::from_mesh(
                                            &context.device,
                                            &new_water,
                                        ));
                                    } else {
                                        chunk_data.water_mesh_buffer = None;
                                    }
                                    chunk_data.dirty = false;
                                    chunk_data.lod_level = target_lod_lvl;
                                    _dirty_done += 1;
                                } // if let Some(chunk_data)
                            } // for chunk_id in dirty_ids
                            // Lazy collider build: chunks close to player that have a mesh but no collider
                            // (were meshed while player was far, now player has approached).
                            // IMPORTANT: compute distance from voxel coords — do NOT call extract_chunk_mesh
                            // just for the center, as that's ~80ms per chunk and would stall every frame.
                            if colliders_built < COLLIDER_PER_FRAME {
                                // Collect candidate IDs first for immutable neighbour lookups.
                                let candidate: Option<ChunkId> = chunk_streamer
                                    .loaded_chunks()
                                    .filter(|c| c.collider.is_none() && c.mesh_buffer.is_some())
                                    .find(|c| {
                                        let min_v = c.id.min_voxel();
                                        let max_v = c.id.max_voxel();
                                        let cx = ((min_v.x + max_v.x) / 2 - origin_voxel.x) as f32;
                                        let cy = ((min_v.y + max_v.y) / 2 - origin_voxel.y) as f32;
                                        let cz = ((min_v.z + max_v.z) / 2 - origin_voxel.z) as f32;
                                        (player_local_pos - Vec3::new(cx, cy, cz)).length()
                                            < COLLIDER_RANGE_M
                                    })
                                    .map(|c| c.id);
                                if let Some(chunk_id) = candidate {
                                    let nx_sc = chunk_streamer
                                        .get_chunk(&ChunkId::new(
                                            chunk_id.x + 1,
                                            chunk_id.y,
                                            chunk_id.z,
                                        ))
                                        .and_then(|c| c.surface_cache.clone());
                                    let nz_sc = chunk_streamer
                                        .get_chunk(&ChunkId::new(
                                            chunk_id.x,
                                            chunk_id.y,
                                            chunk_id.z + 1,
                                        ))
                                        .and_then(|c| c.surface_cache.clone());
                                    let ny_lower_sc = chunk_streamer
                                        .get_chunk(&ChunkId::new(
                                            chunk_id.x,
                                            chunk_id.y - 1,
                                            chunk_id.z,
                                        ))
                                        .and_then(|c| c.surface_cache.clone());
                                    let ny_upper_sc = chunk_streamer
                                        .get_chunk(&ChunkId::new(
                                            chunk_id.x,
                                            chunk_id.y + 1,
                                            chunk_id.z,
                                        ))
                                        .and_then(|c| c.surface_cache.clone());
                                    if let Some(chunk_data) =
                                        chunk_streamer.get_chunk_mut(&chunk_id)
                                    {
                                        let min_v = chunk_data.id.min_voxel();
                                        let max_v = chunk_data.id.max_voxel();
                                        let (mut mesh, chunk_center) =
                                            match &chunk_data.surface_cache {
                                                Some(sc) => extract_chunk_mesh_smooth(
                                                    &chunk_data.octree,
                                                    sc,
                                                    &min_v,
                                                    &max_v,
                                                    nx_sc.as_ref(),
                                                    nz_sc.as_ref(),
                                                    ny_lower_sc.as_ref(),
                                                    ny_upper_sc.as_ref(),
                                                    1,
                                                ),
                                                None => extract_chunk_mesh(
                                                    &chunk_data.octree,
                                                    &min_v,
                                                    &max_v,
                                                    1,
                                                ),
                                            };
                                        let real_offset = Vec3::new(
                                            (chunk_center.x - origin_voxel.x) as f32,
                                            (chunk_center.y - origin_voxel.y) as f32,
                                            (chunk_center.z - origin_voxel.z) as f32,
                                        );
                                        for v in &mut mesh.vertices {
                                            v.position[0] += real_offset.x;
                                            v.position[1] += real_offset.y;
                                            v.position[2] += real_offset.z;
                                        }
                                        if !mesh.vertices.is_empty() {
                                            let col =
                                                metaverse_core::physics::create_collision_from_mesh(
                                                    &mut physics,
                                                    &mesh,
                                                    &origin_voxel,
                                                    None,
                                                );
                                            chunk_data.collider = Some(col);
                                        }
                                    }
                                }
                            }

                            // Render
                            pipeline.update_camera(&context.queue, &camera);
                            billboard_pipeline.update_camera(
                                &context.queue,
                                &camera.build_view_projection_matrix(),
                            );

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
                            if matches!(game_mode, GameMode::Construct)
                                && billboard_frame_counter % 120 == 1
                            {
                                // Determine which module to (re-)build: prefer nearest to player
                                let build_idx = hud_near_module.or_else(|| {
                                    // find the closest content module
                                    let ploc = Vec3::new(
                                        player_local_pos.x as f32,
                                        player_local_pos.y as f32,
                                        player_local_pos.z as f32,
                                    );
                                    MODULE_SECTIONS
                                        .iter()
                                        .enumerate()
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
                                                &context.device,
                                                &context.queue,
                                                &billboard_pipeline,
                                                section.clone(),
                                                items,
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
                                let all_objs: Vec<_> = multiplayer
                                    .all_world_objects()
                                    .filter(|o| matches!(o.object_type, ObjectType::Billboard))
                                    .collect();
                                let current_ids: Vec<String> =
                                    all_objs.iter().map(|o| o.id.clone()).collect();
                                placed_billboards.retain(|(id, _)| current_ids.contains(id));
                                for obj in &all_objs {
                                    if placed_billboards.iter().any(|(id, _)| id == &obj.id) {
                                        continue;
                                    }
                                    let items = multiplayer.get_content(&obj.content_key);
                                    let section = match obj.content_key.as_str() {
                                        "wiki" => metaverse_core::meshsite::Section::Wiki,
                                        "marketplace" => {
                                            metaverse_core::meshsite::Section::Marketplace
                                        }
                                        "post" => metaverse_core::meshsite::Section::Post,
                                        _ => metaverse_core::meshsite::Section::Forums,
                                    };
                                    let mb = ModuleBillboards::build(
                                        &context.device,
                                        &context.queue,
                                        &billboard_pipeline,
                                        section,
                                        items,
                                        obj.pos_vec3(),
                                        obj.facing_normal(),
                                    );
                                    placed_billboards.push((obj.id.clone(), mb));
                                }
                            }

                            if let Some(runner) = capture_runner.as_mut() {
                                capture_runner_post_update(
                                    runner,
                                    &mut player,
                                    &mut physics,
                                    &chunk_streamer,
                                    &mut player_chunk,
                                    game_loading,
                                    origin_gps,
                                    origin_voxel,
                                    &mut layer_view_mode,
                                );
                            }

                            water_pipeline
                                .update_time(&context.queue, render_start.elapsed().as_secs_f32());
                            match context.surface.get_current_texture() {
                                Ok(frame) => {
                                    let view = frame
                                        .texture
                                        .create_view(&wgpu::TextureViewDescriptor::default());

                                    let mut encoder = context.device.create_command_encoder(
                                        &wgpu::CommandEncoderDescriptor {
                                            label: Some("Render"),
                                        },
                                    );

                                    {
                                        let mut render_pass =
                                            pipeline.begin_frame(&mut encoder, &view);
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
                                                MODULE_SECTIONS
                                                    .iter()
                                                    .enumerate()
                                                    .filter(|(_, s)| s.is_some())
                                                    .min_by(|(a, _), (b, _)| {
                                                        let da = (MODULES[*a].door_pos() - ploc)
                                                            .length();
                                                        let db = (MODULES[*b].door_pos() - ploc)
                                                            .length();
                                                        da.partial_cmp(&db).unwrap()
                                                    })
                                                    .map(|(idx, _)| idx)
                                            }) {
                                                if let Some(mb) = &module_billboards[i] {
                                                    billboard_pipeline
                                                        .begin_render(&mut render_pass);
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
                                            // Water: animated semi-transparent surface (rendered after all opaque geometry)
                                            water_pipeline.set_pipeline(&mut render_pass);
                                            render_pass.set_bind_group(
                                                0,
                                                pipeline.camera_bind_group(),
                                                &[],
                                            );
                                            render_pass.set_bind_group(
                                                1,
                                                &pipeline.model_bind_group,
                                                &[],
                                            );
                                            for chunk_data in chunk_streamer.loaded_chunks() {
                                                if let Some(buf) = &chunk_data.water_mesh_buffer {
                                                    buf.render(&mut render_pass);
                                                }
                                            }
                                            // Restore main pipeline for anything that follows
                                            pipeline.set_pipeline(&mut render_pass);

                                            // Inferred world objects (benches, streetlights, etc.)
                                            if !inferred_objects.is_empty() {
                                                textured_pipeline.set_pipeline(
                                                    &mut render_pass,
                                                    pipeline.camera_bind_group(),
                                                );
                                                for iobj in &inferred_objects {
                                                    if let Some(model) =
                                                        object_models.get(&iobj.model_name)
                                                    {
                                                        TexturedPipeline::draw_model(
                                                            &mut render_pass,
                                                            model,
                                                            &iobj.bind_group,
                                                        );
                                                    }
                                                }
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
                                        pipeline.set_model_bind_group(
                                            &mut render_pass,
                                            &player_model_bind_group,
                                        );
                                        hitbox_buffer.render(&mut render_pass);

                                        // Render all remote players
                                        let mut rendered_count = 0;
                                        for remote in multiplayer.remote_players() {
                                            if let Some((_, bind_group)) =
                                                remote_player_bind_groups.get(&remote.peer_id)
                                            {
                                                pipeline.set_model_bind_group(
                                                    &mut render_pass,
                                                    bind_group,
                                                );
                                                remote_player_buffer.render(&mut render_pass);
                                                rendered_count += 1;
                                            }
                                        }

                                        if frame_count % 60 == 0 && rendered_count > 0 {
                                            println!(
                                                "🖼️  Actually rendered {} remote players",
                                                rendered_count
                                            );
                                        }

                                        // Render crosshair (last, on top)
                                        if capture_runner
                                            .as_ref()
                                            .map(|runner| !runner.is_running())
                                            .unwrap_or(true)
                                        {
                                            pipeline.set_model_bind_group(
                                                &mut render_pass,
                                                &crosshair_bind_group,
                                            );
                                            crosshair_buffer.render(&mut render_pass);
                                        }
                                    }

                                    context.queue.submit(std::iter::once(encoder.finish()));

                                    // Signup overlay — rendered on top of the 3D world
                                    // into the same texture view before presenting.
                                    if let Some(ref mut s) = signup {
                                        if let Some((key_type, name, email)) =
                                            s.render(&context, &view, &window)
                                        {
                                            // LoadKey path reuses `name` as the file path.
                                            if key_type == KeyType::User
                                                && name
                                                    .as_deref()
                                                    .map(|p| p.contains(".key"))
                                                    .unwrap_or(false)
                                            {
                                                // Returning user — load key from path (future: implement file load)
                                                println!(
                                                    "🔑 Load key from: {}",
                                                    name.as_deref().unwrap_or("?")
                                                );
                                                signup = None;
                                            } else if let Err(e) =
                                                identity.save_with_type(key_type, name, None)
                                            {
                                                eprintln!("⚠️  Failed to save identity: {}", e);
                                            } else {
                                                println!("✅ Identity saved ({:?})", key_type);
                                                if let Some(e) = email {
                                                    println!(
                                                        "   Email (verification pending): {}",
                                                        e
                                                    );
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
                                                println!(
                                                    "📤 Published [{:?}] \"{}\"",
                                                    item.section, item.title
                                                );
                                                // Trigger immediate billboard refresh so post appears on wall
                                                billboard_frame_counter = 0;
                                                compose = None;
                                                // Restore mouse grab
                                                let _ = window.set_cursor_grab(
                                                    winit::window::CursorGrabMode::Locked,
                                                );
                                                window.set_cursor_visible(false);
                                                cursor_grabbed = true;
                                            }
                                            ComposeResult::Cancel => {
                                                compose = None;
                                                // Restore mouse grab
                                                let _ = window.set_cursor_grab(
                                                    winit::window::CursorGrabMode::Locked,
                                                );
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
                                            let _ = window.set_cursor_grab(
                                                winit::window::CursorGrabMode::Locked,
                                            );
                                            window.set_cursor_visible(false);
                                            cursor_grabbed = true;
                                        }
                                    }

                                    // Debug HUD — always visible, top-left corner
                                    let mode_str = match &game_mode {
                                        GameMode::Construct => "Construct",
                                        GameMode::OpenWorld => "Open World",
                                    };
                                    let observability_probe = if game_mode == GameMode::OpenWorld {
                                        Some(build_observability_probe(
                                            &chunk_streamer,
                                            &player.position,
                                            physics.ecef_to_local(&player.position),
                                        ))
                                    } else {
                                        None
                                    };
                                    let active_chunk_layer = if game_mode == GameMode::OpenWorld
                                        && layer_view_mode != LayerViewMode::Off
                                    {
                                        let active_chunk = ChunkId::from_ecef(&player.position);
                                        if let Some(chunk) = chunk_streamer.get_chunk(&active_chunk)
                                        {
                                            let rebuild = active_chunk_layer_summary
                                                .as_ref()
                                                .map_or(true, |summary| {
                                                    summary.chunk_id != active_chunk
                                                        || summary.last_modified
                                                            != chunk.last_modified
                                                });
                                            if rebuild {
                                                active_chunk_layer_summary =
                                                    Some(build_chunk_layer_summary(chunk));
                                            }
                                            active_chunk_layer_summary.as_ref()
                                        } else {
                                            active_chunk_layer_summary = None;
                                            None
                                        }
                                    } else {
                                        None
                                    };
                                    // Only show module proximity prompt when in Construct walking mode
                                    let near_module_hud =
                                        if matches!(game_mode, GameMode::Construct) {
                                            hud_near_module
                                        } else {
                                            None
                                        };
                                    let show_basic_hud = capture_runner
                                        .as_ref()
                                        .map(|runner| !runner.should_hide_basic_hud())
                                        .unwrap_or(true);
                                    let player_local = physics.ecef_to_local(&player.position);
                                    let player_gps = open_world_local_to_gps(
                                        player_local,
                                        origin_gps,
                                        origin_voxel,
                                    );
                                    let player_orthometric_alt_m =
                                        gps_orthometric_alt_m(player_gps);
                                    let compass_bearing_deg = open_world_forward_bearing_deg(
                                        player_local,
                                        player.camera_forward(),
                                        origin_gps,
                                        origin_voxel,
                                    )
                                    .unwrap_or(0.0);
                                    hud.render(
                                        &context,
                                        &view,
                                        &window,
                                        show_basic_hud,
                                        mode_str,
                                        (player_gps.lat, player_gps.lon, player_orthometric_alt_m),
                                        compass_bearing_deg,
                                        dist_portal,
                                        dist_terminal,
                                        near_portal,
                                        hud_near_terminal,
                                        near_module_hud,
                                        observability_mode,
                                        observability_probe.as_ref(),
                                        layer_view_mode,
                                        active_chunk_layer,
                                    );

                                    if queued_frame_capture.is_none() {
                                        if let Some(runner) = capture_runner.as_ref() {
                                            if runner.stage == CaptureStage::CaptureRequested {
                                                queued_frame_capture = runner
                                                    .build_capture_request(
                                                        &player,
                                                        &physics,
                                                        origin_gps,
                                                        origin_voxel,
                                                        observability_probe.as_ref(),
                                                    );
                                            }
                                        }
                                    }

                                    if let Some(request) = queued_frame_capture.take() {
                                        let FrameCaptureRequest {
                                            output_path,
                                            log_label,
                                            record,
                                        } = request;
                                        let is_route_capture = record.is_some();
                                        match capture_surface_texture(
                                            &context,
                                            &frame.texture,
                                            &output_path,
                                        ) {
                                            Ok(()) => {
                                                println!(
                                                    "📸 Saved {} → {}",
                                                    log_label,
                                                    output_path.display()
                                                );
                                                if is_route_capture {
                                                    if let Some(runner) = capture_runner.as_mut() {
                                                        if let Err(err) =
                                                            runner.record_capture(record)
                                                        {
                                                            runner.fail(err);
                                                        }
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                eprintln!(
                                                    "📸 Screenshot failed for {}: {}",
                                                    log_label, err
                                                );
                                                if is_route_capture {
                                                    if let Some(runner) = capture_runner.as_mut() {
                                                        runner.fail(err);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    frame.present();

                                    if let Some(runner) = capture_runner.as_ref() {
                                        if matches!(
                                            runner.stage,
                                            CaptureStage::Complete | CaptureStage::Failed
                                        ) {
                                            if runner.stage == CaptureStage::Complete {
                                                println!(
                                                    "📸 Capture route '{}' finished — {} image(s) written to {}",
                                                    runner.route.name,
                                                    runner.captures.len(),
                                                    runner.output_dir.display(),
                                                );
                                            }
                                            elwt.exit();
                                            return;
                                        }
                                    }
                                }
                                Err(e) => eprintln!("Surface error: {:?}", e),
                            }

                            // FPS counter and stats
                            frame_count += 1;
                            if fps_timer.elapsed().as_secs() >= 1 {
                                let peer_count = multiplayer.peer_count();

                                println!(
                                    "FPS: {} | Peers: {} | Local: ({:.1}, {:.1}, {:.1}) | Mode: {:?}",
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
                                    println!(
                                        "   Player states: sent={}, received={}",
                                        stats.player_states_sent, stats.player_states_received
                                    );
                                    println!(
                                        "   Voxel ops: sent={}, received={}, applied={}, rejected={}",
                                        stats.voxel_ops_sent,
                                        stats.voxel_ops_received,
                                        stats.voxel_ops_applied,
                                        stats.voxel_ops_rejected
                                    );
                                    println!("   Invalid signatures: {}", stats.invalid_signatures);
                                    println!("   Total messages: {}\n", stats.messages_received);
                                }
                                last_stats_print = Instant::now();
                            }
                        }

                        _ => {}
                    },

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
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new(w, -h, -w), color));
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new(w, -h, w), color));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h, w), color));

    // Top face
    let v4 = mesh.add_vertex(Vertex::new(Vec3::new(-w, h, -w), color));
    let v5 = mesh.add_vertex(Vertex::new(Vec3::new(w, h, -w), color));
    let v6 = mesh.add_vertex(Vertex::new(Vec3::new(w, h, w), color));
    let v7 = mesh.add_vertex(Vertex::new(Vec3::new(-w, h, w), color));

    // Wireframe edges
    mesh.add_line(v0, v1);
    mesh.add_line(v1, v2);
    mesh.add_line(v2, v3);
    mesh.add_line(v3, v0);
    mesh.add_line(v4, v5);
    mesh.add_line(v5, v6);
    mesh.add_line(v6, v7);
    mesh.add_line(v7, v4);
    mesh.add_line(v0, v4);
    mesh.add_line(v1, v5);
    mesh.add_line(v2, v6);
    mesh.add_line(v3, v7);

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
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new(size, 0.0, 0.0), color));
    mesh.add_line(v0, v1);

    // Vertical line
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new(0.0, -size, 0.0), color));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(0.0, size, 0.0), color));
    mesh.add_line(v2, v3);

    mesh
}

// ─── Loading screen ──────────────────────────────────────────────────────────
