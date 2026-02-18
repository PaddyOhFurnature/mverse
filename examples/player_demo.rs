//! Player Demo - Real terrain with visible player model
//! 
//! Features:
//! - Real 100m x 100m terrain from Brisbane
//! - Visible player model (cube)
//! - Walk mode: WASD + mouse look + physics (like Minecraft survival)
//! - Fly mode: WASD + mouse look, free movement (like Minecraft creative)
//! - F key to toggle between modes

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource, OpenTopographySource},
    marching_cubes::extract_octree_mesh,
    mesh::{Mesh, Vertex, Triangle},
    renderer::{Camera, MeshBuffer, RenderContext, RenderPipeline},
    terrain::TerrainGenerator,
    voxel::Octree,
    voxel::VoxelCoord,
};
use glam::{Vec3, Mat4};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    event::*,
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlayerMode {
    Walk,  // Physics-based, can walk/jump/run
    Fly,   // Free movement, no gravity
}

struct Player {
    position: Vec3,      // Player position in local space
    velocity: Vec3,      // Current velocity
    yaw: f32,           // Camera yaw (radians)
    pitch: f32,         // Camera pitch (radians)
    mode: PlayerMode,
    on_ground: bool,
}

impl Player {
    fn new(start_pos: Vec3) -> Self {
        Self {
            position: start_pos,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            mode: PlayerMode::Walk,
            on_ground: false,
        }
    }
    
    fn camera_position(&self) -> Vec3 {
        // Camera at player position + eye height (1.6m)
        self.position + Vec3::new(0.0, 1.6, 0.0)
    }
    
    fn forward_direction(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos(),
            0.0,
            self.yaw.sin()
        ).normalize()
    }
    
    fn right_direction(&self) -> Vec3 {
        Vec3::new(
            (self.yaw + std::f32::consts::PI / 2.0).cos(),
            0.0,
            (self.yaw + std::f32::consts::PI / 2.0).sin()
        ).normalize()
    }
    
    fn look_direction(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos()
        ).normalize()
    }
}

fn main() {
    env_logger::init();
    
    println!("=== PLAYER DEMO ===");
    println!("Building real terrain with visible player model\n");
    
    // STEP 1: Initialize window and renderer
    println!("STEP 1: Initialize window...");
    let event_loop = EventLoop::new().unwrap();
    let window = event_loop
        .create_window(
            winit::window::WindowAttributes::default()
                .with_title("Player Demo - Metaverse Core")
                .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080))
        )
        .unwrap();
    let window = Arc::new(window);
    
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let mut pipeline = RenderPipeline::new(&context);
    println!("✅ Renderer initialized\n");
    
    // STEP 2: Generate real terrain (100m x 100m)
    println!("STEP 2: Generate terrain (100m × 100m)...");
    let start = Instant::now();
    
    let mut elevation_pipeline = ElevationPipeline::new();
    
    // Try NAS SRTM file first  
    if let Some(nas_source) = NasFileSource::new() {
        println!("Using NAS SRTM file");
        elevation_pipeline.add_source(Box::new(nas_source));
    } else {
        println!("Using OpenTopography API");
    }
    
    // Add API as fallback
    let api_key = "3e607de6969c687053f9e107a4796962".to_string();
    let cache_dir = PathBuf::from("./elevation_cache");
    let api = OpenTopographySource::new(api_key, cache_dir);
    elevation_pipeline.add_source(Box::new(api));
    
    let mut generator = TerrainGenerator::new(elevation_pipeline);
    let mut octree = Octree::new();
    
    // Brisbane location (known working)
    let origin = GPS::new(-27.4775, 153.0355, 0.0);
    generator.generate_region(&mut octree, &origin, 100.0)
        .expect("Failed to generate terrain");
    
    let origin_ecef = origin.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    
    println!("✅ Terrain generated in {:.2}s\n", start.elapsed().as_secs_f32());
    
    // STEP 3: Extract mesh
    println!("STEP 3: Extract mesh...");
    let mesh_start = Instant::now();
    let terrain_mesh = extract_octree_mesh(&octree, &origin_voxel, 7); // 128 voxel region
    
    println!("✅ Mesh extracted in {:.2}s", mesh_start.elapsed().as_secs_f32());
    println!("  Vertices: {}", terrain_mesh.vertices.len());
    println!("  Triangles: {}", terrain_mesh.triangles.len());
    
    if terrain_mesh.vertices.is_empty() {
        eprintln!("❌ ERROR: No mesh generated!");
        return;
    }
    
    let mut terrain_buffer = MeshBuffer::from_mesh(&context.device, &terrain_mesh);
    println!();
    
    // STEP 4: Create player model (visible cube)
    println!("STEP 4: Create player cube...");
    let player_mesh = create_player_cube();
    let player_buffer = MeshBuffer::from_mesh(&context.device, &player_mesh);
    println!("✅ Player cube created (0.6m × 1.8m × 0.6m)\n");
    
    // STEP 5: Find ground level and spawn player
    println!("STEP 4: Spawn player...");
    
    // Scan from origin downward to find first solid voxel
    let mut ground_y = origin_voxel.y;
    for y_offset in -20..20 {
        let test_coord = VoxelCoord::new(origin_voxel.x, origin_voxel.y + y_offset, origin_voxel.z);
        if octree.get_voxel(test_coord) != metaverse_core::materials::MaterialId::AIR {
            ground_y = origin_voxel.y + y_offset;
            break;
        }
    }
    
    // Spawn 3 meters above ground in local coordinates
    let spawn_local_y = (ground_y - origin_voxel.y) as f32 + 3.0;
    let mut player = Player::new(Vec3::new(0.0, spawn_local_y, 0.0));
    
    println!("✅ Player spawned at local Y={:.1} (3m above ground at Y={})", spawn_local_y, ground_y);
    println!();
    
    // STEP 6: Create model transform bind group for player
    let player_model_matrix = Mat4::from_translation(player.position);
    let (player_model_buffer, player_model_bind_group) = pipeline.create_model_bind_group(&context.device, &player_model_matrix);
    
    // STEP 7: Setup camera
    // STEP 7: Setup camera
    println!("STEP 7: Setup camera...");
    let aspect = context.size.width as f32 / context.size.height as f32;
    let mut camera = Camera::new(player.camera_position(), aspect);
    camera.yaw = player.yaw;
    camera.pitch = player.pitch;
    println!("✅ Camera positioned at player eyes\n");
    
    // Input state
    let mut move_forward = 0.0f32;
    let mut move_right = 0.0f32;
    let mut move_up = 0.0f32;
    let mut jump_pressed = false;
    let mut sprint_pressed = false;
    let mut cursor_grabbed = false;
    
    let mut frame_count = 0;
    let mut fps_timer = Instant::now();
    let mut last_frame_time = Instant::now();
    
    println!("========================================");
    println!("🎮 DEMO RUNNING");
    println!("Controls:");
    println!("  WASD - Move");
    println!("  Space - Jump (walk mode) / Up (fly mode)");
    println!("  Shift - Sprint (walk mode) / Down (fly mode)");
    println!("  F - Toggle Walk/Fly mode");
    println!("  Mouse - Look around");
    println!("  Left Click - Grab mouse");
    println!("  ESC - Quit");
    println!("========================================\n");
    
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { ref event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested
                | WindowEvent::KeyboardInput {
                    event: KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        ..
                    },
                    ..
                } => elwt.exit(),
                
                WindowEvent::Resized(physical_size) => {
                    context.resize(*physical_size);
                    pipeline.resize(&context.device, &context.config);
                    camera.resize(physical_size.width, physical_size.height);
                }
                
                WindowEvent::KeyboardInput {
                    event: KeyEvent { physical_key, state, .. },
                    ..
                } => {
                    let pressed = *state == ElementState::Pressed;
                    if let PhysicalKey::Code(code) = physical_key {
                        match code {
                            KeyCode::KeyW => move_forward = if pressed { 1.0 } else { 0.0 },
                            KeyCode::KeyS => move_forward = if pressed { -1.0 } else { 0.0 },
                            KeyCode::KeyA => move_right = if pressed { -1.0 } else { 0.0 },
                            KeyCode::KeyD => move_right = if pressed { 1.0 } else { 0.0 },
                            KeyCode::Space => {
                                if player.mode == PlayerMode::Walk {
                                    jump_pressed = pressed;
                                } else {
                                    move_up = if pressed { 1.0 } else { 0.0 };
                                }
                            }
                            KeyCode::ShiftLeft => {
                                if player.mode == PlayerMode::Walk {
                                    sprint_pressed = pressed;
                                } else {
                                    move_up = if pressed { -1.0 } else { 0.0 };
                                }
                            }
                            KeyCode::KeyF if pressed => {
                                // Toggle mode
                                player.mode = match player.mode {
                                    PlayerMode::Walk => PlayerMode::Fly,
                                    PlayerMode::Fly => PlayerMode::Walk,
                                };
                                player.velocity = Vec3::ZERO; // Reset velocity on mode change
                                println!("Mode: {:?}", player.mode);
                            }
                            _ => {}
                        }
                    }
                }
                
                WindowEvent::MouseInput { state, button, .. } => {
                    if *button == MouseButton::Left && *state == ElementState::Pressed {
                        window.set_cursor_visible(false);
                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                        cursor_grabbed = true;
                    }
                }
                
                WindowEvent::RedrawRequested => {
                    let dt = last_frame_time.elapsed().as_secs_f32().min(0.1);
                    last_frame_time = Instant::now();
                    
                    // Update player physics/movement
                    match player.mode {
                        PlayerMode::Walk => {
                            // Walk mode: physics-based movement
                            update_walk_mode(&mut player, move_forward, move_right, jump_pressed, sprint_pressed, dt);
                        }
                        PlayerMode::Fly => {
                            // Fly mode: free movement in look direction
                            update_fly_mode(&mut player, move_forward, move_right, move_up, dt);
                        }
                    }
                    
                    // Update camera to follow player
                    camera.position = player.camera_position();
                    camera.yaw = player.yaw;
                    camera.pitch = player.pitch;
                    
                    // Update player model transform
                    let player_model_matrix = Mat4::from_translation(player.position);
                    context.queue.write_buffer(&player_model_buffer, 0, bytemuck::cast_slice(player_model_matrix.as_ref()));
                    
                    // Render
                    pipeline.update_camera(&context.queue, &camera);
                    
                    match context.surface.get_current_texture() {
                        Ok(frame) => {
                            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                            let mut encoder = context.device.create_command_encoder(
                                &wgpu::CommandEncoderDescriptor { label: Some("Render") }
                            );
                            
                            {
                                let mut render_pass = pipeline.begin_frame(&mut encoder, &view);
                                pipeline.set_pipeline(&mut render_pass);
                                
                                // Render terrain (with identity transform)
                                terrain_buffer.render(&mut render_pass);
                                
                                // Render player model (with player position transform)
                                pipeline.set_model_bind_group(&mut render_pass, &player_model_bind_group);
                                player_buffer.render(&mut render_pass);
                            }
                            
                            context.queue.submit(std::iter::once(encoder.finish()));
                            frame.present();
                        }
                        Err(e) => eprintln!("Surface error: {:?}", e),
                    }
                    
                    // FPS counter
                    frame_count += 1;
                    if fps_timer.elapsed().as_secs() >= 1 {
                        println!("FPS: {} | Mode: {:?} | Pos: ({:.1}, {:.1}, {:.1}) | Vel: {:.1} m/s",
                            frame_count,
                            player.mode,
                            player.position.x,
                            player.position.y,
                            player.position.z,
                            player.velocity.length()
                        );
                        frame_count = 0;
                        fps_timer = Instant::now();
                    }
                }
                
                _ => {}
            }
            
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                if cursor_grabbed {
                    let sensitivity = 0.002;
                    player.yaw += (delta.0 as f32) * sensitivity;
                    player.pitch -= (delta.1 as f32) * sensitivity;
                    player.pitch = player.pitch.clamp(-1.5, 1.5);
                }
            }
            
            Event::AboutToWait => {
                window.request_redraw();
            }
            
            _ => {}
        }
    }).unwrap();
}

fn update_walk_mode(player: &mut Player, forward: f32, right: f32, jump: bool, sprint: bool, dt: f32) {
    const WALK_SPEED: f32 = 4.5; // m/s
    const SPRINT_SPEED: f32 = 7.0; // m/s
    const JUMP_SPEED: f32 = 5.0; // m/s
    const GRAVITY: f32 = 9.8; // m/s²
    const ACCEL: f32 = 20.0; // m/s²
    
    let speed = if sprint { SPRINT_SPEED } else { WALK_SPEED };
    
    // Calculate movement direction
    let forward_dir = player.forward_direction();
    let right_dir = player.right_direction();
    let move_dir = (forward_dir * forward + right_dir * right).normalize_or_zero();
    
    // Horizontal velocity
    let target_horizontal = move_dir * speed;
    let current_horizontal = Vec3::new(player.velocity.x, 0.0, player.velocity.z);
    let new_horizontal = current_horizontal + (target_horizontal - current_horizontal).clamp_length_max(ACCEL * dt);
    
    // Vertical velocity (gravity)
    let mut vertical_velocity = player.velocity.y;
    
    // Simple ground detection (check if close to ground level)
    // TODO: Proper collision detection
    player.on_ground = player.position.y <= -17.0 + 0.9; // Half height of player (1.8m / 2)
    
    if player.on_ground {
        vertical_velocity = 0.0;
        player.position.y = -17.0 + 0.9; // Snap to ground
        
        if jump {
            vertical_velocity = JUMP_SPEED;
            player.on_ground = false;
        }
    } else {
        vertical_velocity -= GRAVITY * dt;
    }
    
    // Update velocity and position
    player.velocity = Vec3::new(new_horizontal.x, vertical_velocity, new_horizontal.z);
    player.position += player.velocity * dt;
}

fn update_fly_mode(player: &mut Player, forward: f32, right: f32, up: f32, dt: f32) {
    const FLY_SPEED: f32 = 10.0; // m/s
    const FLY_ACCEL: f32 = 30.0; // m/s²
    
    // Movement in look direction (full 3D)
    let look_dir = player.look_direction();
    let right_dir = player.right_direction();
    let up_dir = Vec3::Y;
    
    let move_dir = (look_dir * forward + right_dir * right + up_dir * up).normalize_or_zero();
    
    // Accelerate toward target velocity
    let target_velocity = move_dir * FLY_SPEED;
    let velocity_delta = (target_velocity - player.velocity).clamp_length_max(FLY_ACCEL * dt);
    player.velocity += velocity_delta;
    
    // Update position
    player.position += player.velocity * dt;
    
    player.on_ground = false;
}

// Player model rendering will be added in next iteration
// For now, player is invisible but camera follows player position

fn create_player_cube() -> Mesh {
    // Create a 0.6m × 1.8m × 0.6m cube (player dimensions like Minecraft)
    // Centered at origin, will be translated to player position
    let w = 0.3; // Half width (0.6m total)
    let h = 0.9; // Half height (1.8m total)
    
    let mut mesh = Mesh::new();
    
    // Bottom face (Y = -h)
    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h, -w), Vec3::new(0.0, -1.0, 0.0)));
    let v1 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h, -w), Vec3::new(0.0, -1.0, 0.0)));
    let v2 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h,  w), Vec3::new(0.0, -1.0, 0.0)));
    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h,  w), Vec3::new(0.0, -1.0, 0.0)));
    mesh.add_triangle(Triangle::new(v0, v2, v1));
    mesh.add_triangle(Triangle::new(v0, v3, v2));
    
    // Top face (Y = +h)
    let v4 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h, -w), Vec3::new(0.0, 1.0, 0.0)));
    let v5 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h, -w), Vec3::new(0.0, 1.0, 0.0)));
    let v6 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h,  w), Vec3::new(0.0, 1.0, 0.0)));
    let v7 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h,  w), Vec3::new(0.0, 1.0, 0.0)));
    mesh.add_triangle(Triangle::new(v4, v5, v6));
    mesh.add_triangle(Triangle::new(v4, v6, v7));
    
    // Front face (Z = -w)
    let v8 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h, -w), Vec3::new(0.0, 0.0, -1.0)));
    let v9 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h, -w), Vec3::new(0.0, 0.0, -1.0)));
    let v10 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h, -w), Vec3::new(0.0, 0.0, -1.0)));
    let v11 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h, -w), Vec3::new(0.0, 0.0, -1.0)));
    mesh.add_triangle(Triangle::new(v8, v9, v10));
    mesh.add_triangle(Triangle::new(v8, v10, v11));
    
    // Back face (Z = +w)
    let v12 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h,  w), Vec3::new(0.0, 0.0, 1.0)));
    let v13 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h,  w), Vec3::new(0.0, 0.0, 1.0)));
    let v14 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h,  w), Vec3::new(0.0, 0.0, 1.0)));
    let v15 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h,  w), Vec3::new(0.0, 0.0, 1.0)));
    mesh.add_triangle(Triangle::new(v12, v14, v13));
    mesh.add_triangle(Triangle::new(v12, v15, v14));
    
    // Left face (X = -w)
    let v16 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h, -w), Vec3::new(-1.0, 0.0, 0.0)));
    let v17 = mesh.add_vertex(Vertex::new(Vec3::new(-w, -h,  w), Vec3::new(-1.0, 0.0, 0.0)));
    let v18 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h,  w), Vec3::new(-1.0, 0.0, 0.0)));
    let v19 = mesh.add_vertex(Vertex::new(Vec3::new(-w,  h, -w), Vec3::new(-1.0, 0.0, 0.0)));
    mesh.add_triangle(Triangle::new(v16, v17, v18));
    mesh.add_triangle(Triangle::new(v16, v18, v19));
    
    // Right face (X = +w)
    let v20 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h, -w), Vec3::new(1.0, 0.0, 0.0)));
    let v21 = mesh.add_vertex(Vertex::new(Vec3::new( w, -h,  w), Vec3::new(1.0, 0.0, 0.0)));
    let v22 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h,  w), Vec3::new(1.0, 0.0, 0.0)));
    let v23 = mesh.add_vertex(Vertex::new(Vec3::new( w,  h, -w), Vec3::new(1.0, 0.0, 0.0)));
    mesh.add_triangle(Triangle::new(v20, v22, v21));
    mesh.add_triangle(Triangle::new(v20, v23, v22));
    
    mesh
}
