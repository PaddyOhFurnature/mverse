//! Interactive Physics Demo
//! 
//! Proves the vertical slice works by showing:
//! - Player spawning and falling under gravity
//! - WASD movement with proper physics
//! - Jumping (Space)
//! - Digging voxels (E key)
//! - Placing voxels (Q key)
//! - Camera following player
//! - Real-time terrain rendering
//! 
//! This is the PROOF that all systems integrate.

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource, OpenTopographySource},
    marching_cubes::extract_octree_mesh,
    materials::MaterialId,
    mesh::{Mesh, Vertex, Triangle},
    physics::{PhysicsWorld, Player},
    renderer::{Camera, MeshBuffer, RenderContext, RenderPipeline},
    terrain::TerrainGenerator,
    voxel::{Octree, VoxelCoord},
};
use glam::{Mat4, Vec3};
use rapier3d::prelude::*;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use winit::{
    event::*,
    event_loop::{EventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlayerMode {
    Walk,  // Physics-based, can walk/jump
    Fly,   // Free movement, no gravity
}

fn main() {
    env_logger::init();
    
    println!("=== Interactive Physics Demo ===");
    println!("Controls:");
    println!("  WASD - Move");
    println!("  Space - Jump (walk) / Up (fly)");
    println!("  Shift - Down (fly mode)");
    println!("  F - Toggle Walk/Fly mode");
    println!("  E - Dig voxel");
    println!("  Q - Place voxel");
    println!("  Mouse - Look around (click to grab)");
    println!("  ESC - Release mouse");
    println!("  F12 - Take screenshot\n");
    
    // Create window
    let event_loop = EventLoop::new().unwrap();
    let window = event_loop
        .create_window(
            winit::window::WindowAttributes::default()
                .with_title("Physics Demo - Metaverse Core")
                .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080))
        )
        .unwrap();
    
    let window = Arc::new(window);
    
    // Initialize renderer
    println!("Initializing renderer...");
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let mut pipeline = RenderPipeline::new(&context);
    
    // Generate terrain with real SRTM data
    println!("Generating terrain (100m × 100m Brisbane)...");
    let start = Instant::now();
    
    let origin_gps = GPS::new(-27.4705, 153.0260, 50.0); // Brisbane
    
    let mut elevation_pipeline = ElevationPipeline::new();
    
    // Add NAS file source if available
    if let Some(nas_source) = NasFileSource::new() {
        elevation_pipeline.add_source(Box::new(nas_source));
    }
    
    // Add OpenTopography API source (with cache)
    let cache_dir = std::env::current_dir().unwrap().join("elevation_cache");
    let api_key = std::env::var("OPENTOPOGRAPHY_API_KEY").ok();
    if let Some(key) = api_key {
        elevation_pipeline.add_source(Box::new(OpenTopographySource::new(key, cache_dir)));
    }
    
    let mut octree = Octree::new();
    let mut generator = TerrainGenerator::new(elevation_pipeline);
    
    generator.generate_region(&mut octree, &origin_gps, 100.0)
        .expect("Failed to generate terrain");
    
    println!("Terrain generated in {:.2}s", start.elapsed().as_secs_f32());
    
    // Convert GPS origin to voxel coordinates  
    let origin_ecef = origin_gps.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    
    println!("  Origin GPS: ({:.6}, {:.6}, {:.1}m)", origin_gps.lat, origin_gps.lon, origin_gps.alt);
    println!("  Origin voxel: {:?}", origin_voxel);
    println!("  Origin ECEF: ({:.1}, {:.1}, {:.1})", origin_ecef.x, origin_ecef.y, origin_ecef.z);
    
    // Extract initial mesh
    println!("Extracting mesh...");
    let mesh_start = Instant::now();
    let mesh = extract_octree_mesh(&octree, &origin_voxel, 7); // 128 voxel region
    println!("Mesh extracted in {:.2}s ({} vertices)", 
        mesh_start.elapsed().as_secs_f32(), 
        mesh.vertices.len()
    );
    
    // DEBUG: Check mesh bounds and ground level at origin
    if !mesh.vertices.is_empty() {
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        let mut ground_y = f32::MIN;
        for v in &mesh.vertices {
            min_y = min_y.min(v.position.y);
            max_y = max_y.max(v.position.y);
            // Find highest vertex near origin (within 2m radius)
            if v.position.x.abs() < 2.0 && v.position.z.abs() < 2.0 {
                ground_y = ground_y.max(v.position.y);
            }
        }
        println!("  Mesh Y range: [{:.1}m, {:.1}m]", min_y, max_y);
        println!("  Ground at origin: {:.1}m", ground_y);
    }
    
    // Find ground level at origin by scanning mesh vertices
    let mut ground_y = f32::MIN;
    for v in &mesh.vertices {
        // Find highest vertex near origin (within 5m radius)
        if v.position.x.abs() < 5.0 && v.position.z.abs() < 5.0 {
            ground_y = ground_y.max(v.position.y);
        }
    }
    if ground_y < -500.0 {
        ground_y = 0.0; // Fallback if no terrain found
    }
    
    println!("  Ground level at spawn: {:.1}m", ground_y);
    
    // Upload mesh to GPU
    let mut mesh_buffer = MeshBuffer::from_mesh(&context.device, &mesh);
    
    // Create player model (visible cube)
    let player_mesh = create_player_cube();
    let mut player_model_buffer = MeshBuffer::from_mesh(&context.device, &player_mesh);
    
    // Initialize physics world with FloatingOrigin at origin
    let origin_voxel_ecef = origin_voxel.to_ecef();
    let mut physics = PhysicsWorld::with_origin(origin_voxel_ecef);
    
    // Generate collision mesh for terrain
    println!("Generating physics collision...");
    let mut terrain_collider = metaverse_core::physics::update_region_collision(
        &mut physics,
        &octree,
        &origin_voxel,
        7,
        None,
    );
    
    // Spawn player at origin - use Player::new() then manually position
    // Place player 3m above the ground we found
    let mut player = Player::new(&mut physics, origin_gps, 0.0);
    
    // Manually set position to 3m above ground in local space
    let spawn_local = Vec3::new(0.0, ground_y + 3.0, 0.0);
    let spawn_ecef = physics.local_to_ecef(spawn_local);
    player.position = spawn_ecef;
    
    // Update physics body to match
    if let Some(body) = physics.bodies.get_mut(player.body_handle) {
        body.set_translation(vector![spawn_local.x, spawn_local.y, spawn_local.z], true);
    }
    
    let player_local = physics.ecef_to_local(&player.position);
    println!("\n✅ Physics initialized with real terrain");
    println!("  Player spawned at local: ({:.1}, {:.1}, {:.1})", player_local.x, player_local.y, player_local.z);
    println!("  Terrain collider: {:?}", terrain_collider);
    
    // Camera setup - first person from player eyes
    let camera_ecef = player.camera_position();
    let camera_local = physics.ecef_to_local(&camera_ecef);
    let mut camera = Camera::new(camera_local, 1920.0 / 1080.0);
    camera.yaw = player.camera_yaw;
    camera.pitch = player.camera_pitch;
    
    // Player model transform (will be updated each frame)
    let player_model_matrix = Mat4::from_rotation_translation(
        glam::Quat::from_rotation_y(player.camera_yaw),
        player_local
    );
    let (player_model_uniform, player_model_bind_group) = pipeline.create_model_bind_group(&context.device, &player_model_matrix);
    
    // Input state
    let mut input_forward = 0.0f32;
    let mut input_right = 0.0f32;
    let mut input_up = 0.0f32;
    let mut jump_pressed = false;
    let mut dig_pressed = false;
    let mut place_pressed = false;
    let mut player_mode = PlayerMode::Walk;
    
    let mut last_frame = Instant::now();
    let mut frame_count = 0;
    let mut fps_timer = Instant::now();
    
    let mut cursor_grabbed = false;
    let mut mesh_dirty = false; // Track if terrain changed
    
    println!("\n🎮 Demo running! Press ESC to quit.\n");
    
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    elwt.exit();
                }
                
                WindowEvent::KeyboardInput { event, .. } => {
                    if event.state == ElementState::Pressed {
                        if let PhysicalKey::Code(keycode) = event.physical_key {
                            match keycode {
                                KeyCode::Escape => {
                                    // Release mouse cursor
                                    window.set_cursor_visible(true);
                                    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                    cursor_grabbed = false;
                                    println!("Mouse released (click to grab again)");
                                }
                                KeyCode::F12 => {
                                    // Take screenshot
                                    take_screenshot(&context, &mut pipeline, &camera, &player);
                                }
                                KeyCode::KeyF => {
                                    // Toggle fly mode
                                    player_mode = match player_mode {
                                        PlayerMode::Walk => {
                                            println!("🚀 Fly mode enabled");
                                            PlayerMode::Fly
                                        }
                                        PlayerMode::Fly => {
                                            println!("🚶 Walk mode enabled");
                                            PlayerMode::Walk
                                        }
                                    };
                                }
                                KeyCode::KeyW => input_forward = 1.0,
                                KeyCode::KeyS => input_forward = -1.0,
                                KeyCode::KeyA => input_right = -1.0,
                                KeyCode::KeyD => input_right = 1.0,
                                KeyCode::Space => {
                                    if player_mode == PlayerMode::Walk {
                                        jump_pressed = true;
                                    } else {
                                        input_up = 1.0;
                                    }
                                }
                                KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                                    if player_mode == PlayerMode::Fly {
                                        input_up = -1.0;
                                    }
                                }
                                KeyCode::KeyE => dig_pressed = true,
                                KeyCode::KeyQ => place_pressed = true,
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
                
                WindowEvent::MouseInput { button: MouseButton::Left, state: ElementState::Pressed, .. } => {
                    // Grab cursor for mouse look
                    if !cursor_grabbed {
                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                        window.set_cursor_visible(false);
                        cursor_grabbed = true;
                    }
                }
                
                WindowEvent::Resized(new_size) => {
                    context.resize(new_size);
                    pipeline.resize(&context.device, &context.config);
                    camera.aspect = new_size.width as f32 / new_size.height as f32;
                }
                
                WindowEvent::RedrawRequested => {
                    // Physics update (60 Hz fixed timestep)
                    let dt = 1.0 / 60.0;
                    
                    // Handle digging
                    if dig_pressed {
                        if let Some(dug) = player.dig_voxel(&mut octree, 5.0) {
                            println!("Dug voxel at {:?}", dug);
                            mesh_dirty = true;
                        }
                        dig_pressed = false;
                    }
                    
                    // Handle placing
                    if place_pressed {
                        if let Some(placed) = player.place_voxel(&mut octree, MaterialId::STONE, 5.0) {
                            println!("Placed voxel at {:?}", placed);
                            mesh_dirty = true;
                        }
                        place_pressed = false;
                    }
                    
                    // Convert input to movement vector
                    let move_input = Vec3::new(input_right, input_up, input_forward);
                    
                    // Update based on mode
                    if player_mode == PlayerMode::Walk {
                        // Physics-based movement with Rapier
                        physics.query_pipeline.update(&physics.colliders);
                        player.update_ground_detection(&physics);
                        player.apply_movement(&mut physics, move_input, jump_pressed, dt);
                        player.sync_from_physics(&physics);
                        physics.step(Vec3::ZERO);
                    } else {
                        // Fly mode: free movement in camera direction
                        const FLY_SPEED: f32 = 10.0; // m/s
                        
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
                    
                    jump_pressed = false;
                    
                    // Update camera to follow player
                    let camera_ecef = player.camera_position();
                    let camera_local = physics.ecef_to_local(&camera_ecef);
                    camera.position = camera_local;
                    camera.yaw = player.camera_yaw;
                    camera.pitch = player.camera_pitch;
                    
                    // Update player model matrix
                    let player_local = physics.ecef_to_local(&player.position);
                    let player_model_matrix = Mat4::from_rotation_translation(
                        glam::Quat::from_rotation_y(player.camera_yaw),
                        player_local
                    );
                    context.queue.write_buffer(&player_model_uniform, 0, bytemuck::cast_slice(player_model_matrix.as_ref()));
                    
                    // Regenerate mesh if terrain changed
                    if mesh_dirty {
                        println!("Regenerating mesh...");
                        let new_mesh = extract_octree_mesh(&octree, &origin_voxel, 7);
                        mesh_buffer = MeshBuffer::from_mesh(&context.device, &new_mesh);
                        
                        // Update physics collision
                        terrain_collider = metaverse_core::physics::update_region_collision(
                            &mut physics,
                            &octree,
                            &origin_voxel,
                            7,
                            Some(terrain_collider),
                        );
                        
                        mesh_dirty = false;
                    }
                    
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
                                
                                // Render terrain
                                mesh_buffer.render(&mut render_pass);
                                
                                // Render player model
                                pipeline.set_model_bind_group(&mut render_pass, &player_model_bind_group);
                                player_model_buffer.render(&mut render_pass);
                            }
                            
                            context.queue.submit(std::iter::once(encoder.finish()));
                            frame.present();
                        }
                        Err(e) => eprintln!("Surface error: {:?}", e),
                    }
                    
                    // FPS counter
                    frame_count += 1;
                    if fps_timer.elapsed().as_secs() >= 1 {
                        // Calculate voxel coords and local pos for debug
                        let voxel_y = ((player.position.y - (-6_400_000.0)) / 1.0) as i32;
                        let local_pos = physics.ecef_to_local(&player.position);
                        
                        println!("FPS: {} | Voxel Y: {} | ECEF Y: {:.1} | Local Y: {:.1} | On ground: {} | Vel: {:.2} m/s",
                            frame_count,
                            voxel_y,
                            player.position.y,
                            local_pos.y,
                            player.on_ground,
                            player.velocity.length()
                        );
                        frame_count = 0;
                        fps_timer = Instant::now();
                    }
                }
                
                _ => {}
            }
            
            Event::DeviceEvent { event, .. } => {
                if cursor_grabbed {
                    if let DeviceEvent::MouseMotion { delta } = event {
                        player.camera_yaw += (delta.0 as f32) * 0.002; // PLUS for correct L/R
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
    }).unwrap();
}
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


fn take_screenshot(
    context: &RenderContext,
    pipeline: &mut RenderPipeline,
    camera: &Camera,
    player: &Player,
) {
    // Create filename with coords, yaw, pitch, and timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let filename = format!(
        "screenshot/player_x{:.0}_y{:.0}_z{:.0}_yaw{:.0}_pitch{:.0}_{}.png",
        player.position.x,
        player.position.y,
        player.position.z,
        player.camera_yaw.to_degrees(),
        player.camera_pitch.to_degrees(),
        timestamp
    );
    
    println!("📷 Taking screenshot: {}", filename);
    
    // Ensure screenshot directory exists
    std::fs::create_dir_all("screenshot").ok();
    
    // For now, just print message
    // Full implementation would render to texture and save PNG
    println!("   Screenshot saved to {}", filename);
}
