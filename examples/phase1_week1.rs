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
    physics::{PhysicsWorld, Player},
    renderer::{Camera, MeshBuffer, RenderContext, RenderPipeline},
    terrain::TerrainGenerator,
    voxel::{Octree, VoxelCoord},
};
use glam::Vec3;
use rapier3d::prelude::*;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    event::*,
    event_loop::{EventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
};

fn main() {
    env_logger::init();
    
    println!("=== Interactive Physics Demo ===");
    println!("Controls:");
    println!("  WASD - Move");
    println!("  Space - Jump");
    println!("  E - Dig voxel");
    println!("  Q - Place voxel");
    println!("  Mouse - Look around");
    println!("  ESC - Quit\n");
    
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
    
    // Input state
    let mut input_forward = 0.0f32;
    let mut input_right = 0.0f32;
    let mut jump_pressed = false;
    let mut dig_pressed = false;
    let mut place_pressed = false;
    
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
                                KeyCode::Escape => elwt.exit(),
                                KeyCode::KeyW => input_forward = 1.0,
                                KeyCode::KeyS => input_forward = -1.0,
                                KeyCode::KeyA => input_right = -1.0,
                                KeyCode::KeyD => input_right = 1.0,
                                KeyCode::Space => jump_pressed = true,
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
                                KeyCode::Space => jump_pressed = false,
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
                    
                    // Convert input to movement vector (camera is fixed for now, so just use world axes)
                    let move_input = Vec3::new(input_right, 0.0, -input_forward);
                    
                    // Update query pipeline for raycasting
                    physics.query_pipeline.update(&physics.colliders);
                    
                    // Update player physics
                    player.update_ground_detection(&physics);
                    player.apply_movement(&mut physics, move_input, jump_pressed, dt);
                    player.sync_from_physics(&physics);
                    
                    // Step physics simulation
                    // NOTE: Player applies its own gravity in apply_movement(),
                    // so we pass zero gravity to avoid double-application
                    physics.step(Vec3::ZERO);
                    
                    // Update camera to follow player
                    let camera_ecef = player.camera_position();
                    let camera_local = physics.ecef_to_local(&camera_ecef);
                    camera.position = camera_local;
                    camera.yaw = player.camera_yaw;
                    camera.pitch = player.camera_pitch;
                    
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
                                mesh_buffer.render(&mut render_pass);
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
                        player.camera_yaw -= (delta.0 as f32) * 0.002;
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
