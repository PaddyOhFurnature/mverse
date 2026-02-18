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
use std::time::{Instant, SystemTime, UNIX_EPOCH};
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
            pitch: -0.3, // Look down slightly so you can see terrain
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
    println!("STEP 5: Spawn player...");
    
    // Find ground by scanning terrain mesh vertices
    // Mesh is in local coordinates centered at (0, 0, 0)
    let mut ground_y: f32 = -1000.0;
    for vertex in &terrain_mesh.vertices {
        if vertex.position.x.abs() < 5.0 && vertex.position.z.abs() < 5.0 {
            ground_y = ground_y.max(vertex.position.y);
        }
    }
    
    if ground_y < -500.0 {
        // No terrain found near origin, spawn at 0
        ground_y = 0.0;
    }
    
    let spawn_y = ground_y + 3.0; // 3m above ground
    println!("  Ground level: {:.1}m", ground_y);
    println!("  Spawn position: (0.0, {:.1}, 0.0)", spawn_y);
    
    let mut player = Player::new(Vec3::new(0.0, spawn_y, 0.0));
    
    println!("✅ Player spawned at local Y={:.1} (3m above ground)", spawn_y);
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
    println!("  F12 - Take screenshot");
    println!("  Mouse - Look around");
    println!("  Left Click - Grab mouse");
    println!("  ESC - Release mouse");
    println!("========================================\n");
    
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { ref event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                
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
                            KeyCode::Escape if pressed => {
                                // Release mouse cursor
                                window.set_cursor_visible(true);
                                let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                cursor_grabbed = false;
                                println!("Mouse released (click to grab again)");
                            }
                            KeyCode::F12 if pressed => {
                                // Take screenshot
                                take_screenshot(
                                    &context,
                                    &mut pipeline,
                                    &mut camera,
                                    &player,
                                    &terrain_buffer,
                                    &player_buffer,
                                    &player_model_bind_group,
                                );
                            }
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
                            update_walk_mode(&mut player, &terrain_mesh, move_forward, move_right, jump_pressed, sprint_pressed, dt);
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
                    
                    // Update player model transform (translate + rotate with yaw)
                    let player_model_matrix = Mat4::from_rotation_translation(
                        glam::Quat::from_rotation_y(player.yaw),
                        player.position,
                    );
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

fn update_walk_mode(player: &mut Player, terrain_mesh: &Mesh, forward: f32, right: f32, jump: bool, sprint: bool, dt: f32) {
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
    
    // Apply gravity
    let mut vertical_velocity = player.velocity.y - GRAVITY * dt;
    
    // Handle jump input (only when on ground)
    if jump && player.on_ground {
        vertical_velocity = JUMP_SPEED;
        player.on_ground = false;
    }
    
    // Update velocity
    player.velocity = Vec3::new(new_horizontal.x, vertical_velocity, new_horizontal.z);
    
    // Calculate new position from horizontal movement
    let new_horizontal_pos = Vec3::new(
        player.position.x + player.velocity.x * dt,
        player.position.y,
        player.position.z + player.velocity.z * dt,
    );
    
    // Check horizontal collision at new XZ position
    let collision_radius = 0.4; // Slightly larger than player width (0.3m half-width)
    let has_horizontal_collision = check_horizontal_collision(
        terrain_mesh,
        new_horizontal_pos.x,
        new_horizontal_pos.z,
        player.position.y,
        collision_radius,
    );
    
    // Apply horizontal movement only if no collision
    if !has_horizontal_collision {
        player.position.x = new_horizontal_pos.x;
        player.position.z = new_horizontal_pos.z;
    } else {
        // Hit a wall - stop horizontal velocity
        player.velocity.x = 0.0;
        player.velocity.z = 0.0;
    }
    
    // Apply vertical movement (always allow falling/jumping)
    player.position.y += player.velocity.y * dt;
    
    // NOW check ground collision (after moving)
    let player_bottom_y = player.position.y - 0.9; // Player cube bottom
    let ground_height = get_ground_height(terrain_mesh, player.position.x, player.position.z, player_bottom_y, 1.0);
    
    if let Some(ground_y) = ground_height {
        // Check if we're penetrating ground OR close to ground
        let distance_to_ground = player_bottom_y - ground_y;
        
        if distance_to_ground <= 0.0 {
            // Below ground - snap up (falling or walking into rising terrain)
            player.position.y = ground_y + 0.9;
            player.velocity.y = 0.0;
            player.on_ground = true;
        } else if distance_to_ground < 0.2 && player.on_ground {
            // Very close to ground and was on ground - snap to follow terrain
            // This handles walking up slopes and bumps
            player.position.y = ground_y + 0.9;
            player.velocity.y = 0.0;
            player.on_ground = true;
        } else if distance_to_ground > 0.5 {
            // More than 0.5m above ground - we're in the air
            player.on_ground = false;
        } else if distance_to_ground > 0.2 {
            // Between 0.2 and 0.5m - transitioning to air
            player.on_ground = false;
        }
        // else: between 0 and 0.2m, falling toward ground, keep current state
    } else {
        // No terrain found - we're in the air
        player.on_ground = false;
    }
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

/// Find ground height at given XZ position by scanning terrain mesh
/// Returns the highest ground BELOW or slightly above the player's current height
fn get_ground_height(terrain_mesh: &Mesh, x: f32, z: f32, current_y: f32, search_radius: f32) -> Option<f32> {
    let mut best_y = None;
    let max_step_height = 0.5; // Maximum step we can auto-climb (0.5m = one voxel)
    
    for vertex in &terrain_mesh.vertices {
        let dx = vertex.position.x - x;
        let dz = vertex.position.z - z;
        let dist_sq = dx * dx + dz * dz;
        
        if dist_sq <= search_radius * search_radius {
            let y = vertex.position.y;
            
            // Only consider ground that's below us or within step height above
            if y <= current_y + max_step_height {
                match best_y {
                    None => best_y = Some(y),
                    Some(current_best) => {
                        // Take the highest ground that's still below/near us
                        if y > current_best && y <= current_y + max_step_height {
                            best_y = Some(y);
                        }
                    }
                }
            }
        }
    }
    
    best_y
}

/// Check if moving to new XZ position would collide with terrain at player height
/// Returns true if collision detected (terrain blocks horizontal movement)
fn check_horizontal_collision(terrain_mesh: &Mesh, x: f32, z: f32, player_y: f32, search_radius: f32) -> bool {
    let player_center = player_y; // Player center height
    let player_top = player_y + 0.9;
    let min_wall_height = 0.3; // Wall must be at least 0.3m above player bottom to block
    
    for vertex in &terrain_mesh.vertices {
        let dx = vertex.position.x - x;
        let dz = vertex.position.z - z;
        let dist_sq = dx * dx + dz * dz;
        
        if dist_sq <= search_radius * search_radius {
            let y = vertex.position.y;
            
            // Only consider terrain in upper half of player (waist to head)
            // This prevents ground from blocking horizontal movement
            if y >= player_center - 0.3 && y <= player_top {
                return true; // Wall collision!
            }
        }
    }
    
    false // No collision
}

fn take_screenshot(
    context: &RenderContext,
    pipeline: &mut RenderPipeline,
    camera: &mut Camera,
    player: &Player,
    terrain_buffer: &MeshBuffer,
    player_buffer: &MeshBuffer,
    player_model_bind_group: &wgpu::BindGroup,
) {
    // Update camera to current player position/rotation
    camera.position = player.position + Vec3::new(0.0, 1.6, 0.0);
    camera.yaw = player.yaw;
    camera.pitch = player.pitch;
    pipeline.update_camera(&context.queue, camera);
    
    // Create filename with timestamp, position, and view angles
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let filename = format!(
        "screenshot/player_x{:.0}_y{:.0}_z{:.0}_yaw{:.0}_pitch{:.0}_{}.png",
        player.position.x,
        player.position.y,
        player.position.z,
        player.yaw.to_degrees(),
        player.pitch.to_degrees(),
        timestamp
    );
    
    println!("📷 Taking screenshot: {}", filename);
    
    // Ensure screenshot directory exists
    std::fs::create_dir_all("screenshot").ok();
    
    let width = context.size.width;
    let height = context.size.height;
    
    // Create texture to render to
    let screenshot_texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Screenshot Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: context.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    
    let screenshot_view = screenshot_texture.create_view(&wgpu::TextureViewDescriptor::default());
    
    // Calculate buffer dimensions (with padding for GPU alignment)
    let bytes_per_pixel = 4; // RGBA8
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) / align * align;
    let buffer_size = (padded_bytes_per_row * height) as u64;
    
    let output_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Screenshot Buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    
    // Render scene to screenshot texture
    let mut encoder = context.device.create_command_encoder(
        &wgpu::CommandEncoderDescriptor { label: Some("Screenshot Encoder") }
    );
    
    {
        let mut render_pass = pipeline.begin_frame(&mut encoder, &screenshot_view);
        pipeline.set_pipeline(&mut render_pass);
        
        // Render terrain
        terrain_buffer.render(&mut render_pass);
        
        // Render player model
        pipeline.set_model_bind_group(&mut render_pass, player_model_bind_group);
        player_buffer.render(&mut render_pass);
    }
    
    // Copy texture to buffer
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &screenshot_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &output_buffer,
            layout: wgpu::ImageDataLayout {
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
    
    // Map buffer and save to file
    let buffer_slice = output_buffer.slice(..);
    buffer_slice.map_async(wgpu::MapMode::Read, |_| {});
    context.device.poll(wgpu::Maintain::Wait);
    
    {
        let data = buffer_slice.get_mapped_range();
        
        // Remove padding from rows
        let mut png_data = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            let row_start = (row * padded_bytes_per_row) as usize;
            let row_end = row_start + (width * bytes_per_pixel) as usize;
            png_data.extend_from_slice(&data[row_start..row_end]);
        }
        
        // Save to PNG
        image::save_buffer(
            &filename,
            &png_data,
            width,
            height,
            image::ColorType::Rgba8,
        )
        .expect("Failed to save screenshot");
    }
    
    output_buffer.unmap();
    
    println!("✅ Screenshot saved: {}", filename);
}
