//! Physics Integration Demo - Built Step by Step
//! 
//! Starting from working terrain_viewer.rs and adding physics piece by piece.
//! Each step is tested before moving to the next.

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource, OpenTopographySource},
    marching_cubes::extract_octree_mesh,
    materials::MaterialId,
    physics::{PhysicsWorld, Player, update_region_collision},
    renderer::{Camera, MeshBuffer, RenderContext, RenderPipeline},
    terrain::TerrainGenerator,
    voxel::{Octree, VoxelCoord},
};
use glam::Vec3;
use rapier3d::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    event::*,
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
};

fn main() {
    env_logger::init();
    
    println!("=== PHYSICS INTEGRATION TEST ===");
    println!("Building step by step from working terrain_viewer\n");
    
    // STEP 1: Create window and renderer (KNOWN WORKING)
    println!("STEP 1: Initialize window and renderer...");
    let event_loop = EventLoop::new().unwrap();
    let window = event_loop
        .create_window(
            winit::window::WindowAttributes::default()
                .with_title("Physics Integration Test")
                .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080))
        )
        .unwrap();
    
    let window = Arc::new(window);
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let mut pipeline = RenderPipeline::new(&context);
    println!("✅ Renderer initialized\n");
    
    // STEP 2: Generate terrain (KNOWN WORKING from terrain_viewer)
    println!("STEP 2: Generate terrain (250m × 250m)...");
    let start = Instant::now();
    
    let nas = NasFileSource::new();
    let api_key = "3e607de6969c687053f9e107a4796962".to_string();
    let cache_dir = PathBuf::from("./elevation_cache");
    let api = OpenTopographySource::new(api_key, cache_dir);
    
    let mut elevation_pipeline = ElevationPipeline::new();
    if let Some(nas_source) = nas {
        println!("Using NAS SRTM file");
        elevation_pipeline.add_source(Box::new(nas_source));
    } else {
        println!("Using OpenTopography API");
    }
    elevation_pipeline.add_source(Box::new(api));
    
    let mut generator = TerrainGenerator::new(elevation_pipeline);
    let mut octree = Octree::new();
    
    // Brisbane location (known working)
    let origin = GPS::new(-27.4775, 153.0355, 0.0);
    generator.generate_region(&mut octree, &origin, 250.0)
        .expect("Failed to generate terrain");
    
    let origin_ecef = origin.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    
    println!("✅ Terrain generated in {:.2}s", start.elapsed().as_secs_f32());
    
    // STEP 3: Find ground level at spawn point (relative to origin_voxel)
    println!("\nSTEP 3: Find ground level at center...");
    let mut ground_y = 0i64;
    
    // Scan from origin_voxel upward to find first solid voxel
    for y_offset in -20..200 {
        let coord = VoxelCoord::new(origin_voxel.x, origin_voxel.y + y_offset, origin_voxel.z);
        if octree.get_voxel(coord) != MaterialId::AIR {
            ground_y = origin_voxel.y + y_offset;
            break;
        }
    }
    
    let spawn_height_offset = 3i64; // 3m above ground
    println!("Origin voxel: {:?}", origin_voxel);
    println!("Ground level: Y={}", ground_y);
    println!("Spawn height: Y={} (3m above ground)", ground_y + spawn_height_offset);
    println!("✅ Spawn point determined\n");
    
    // STEP 4: Extract mesh (KNOWN WORKING from terrain_viewer)
    println!("STEP 4: Extract mesh...");
    let mesh_start = Instant::now();
    let terrain_mesh = extract_octree_mesh(&octree, &origin_voxel, 8);
    
    println!("✅ Mesh extracted in {:.2}s", mesh_start.elapsed().as_secs_f32());
    println!("  Vertices: {}", terrain_mesh.vertices.len());
    println!("  Triangles: {}", terrain_mesh.triangles.len());
    
    if terrain_mesh.vertices.is_empty() {
        eprintln!("\n❌ ERROR: No mesh generated!");
        return;
    }
    
    let mut mesh_buffer = MeshBuffer::from_mesh(&context.device, &terrain_mesh);
    
    // STEP 5: Initialize physics with FloatingOrigin
    println!("\nSTEP 5: Initialize physics world...");
    let mut physics = PhysicsWorld::with_origin(origin_ecef);
    
    println!("Physics world origin: ({:.1}, {:.1}, {:.1})",
        origin_ecef.x, origin_ecef.y, origin_ecef.z);
    println!("✅ Physics world created\n");
    
    // STEP 6: Generate collision mesh
    println!("STEP 6: Generate collision mesh...");
    let collision_start = Instant::now();
    let mut terrain_collider = update_region_collision(
        &mut physics,
        &octree,
        &origin_voxel,
        8,
        None,
    );
    
    println!("✅ Collision mesh generated in {:.2}s", collision_start.elapsed().as_secs_f32());
    println!("  Collider handle: {:?}\n", terrain_collider);
    
    // STEP 7: Spawn player in local coordinates (relative to world origin)
    println!("STEP 7: Create player...");
    
    // Find where ground is in local coordinates
    // Ground voxel Y is 8967651, origin voxel Y is 8967671
    // So ground is (8967651 - 8967671) = -20 voxels below origin in local space
    let ground_offset_y = (ground_y - origin_voxel.y) as f32;
    let spawn_local_y = ground_offset_y + spawn_height_offset as f32;
    
    println!("Ground offset from origin: {:.1}m", ground_offset_y);
    println!("Spawn local Y: {:.1}m ({}m above ground)", spawn_local_y, spawn_height_offset);
    
    // Create player at origin first
    let spawn_gps = GPS::new(-27.4775, 153.0355, 0.0);
    let mut player = Player::new(&mut physics, spawn_gps, 0.0);
    
    // Override to correct local position (X=0, Y=ground+3, Z=0 in local space)
    let spawn_local = Vec3::new(0.0, spawn_local_y, 0.0);
    let spawn_ecef = physics.local_to_ecef(spawn_local);
    player.position = spawn_ecef;
    
    // Update rigidbody
    if let Some(body) = physics.bodies.get_mut(player.body_handle) {
        body.set_translation(vector![spawn_local.x, spawn_local.y, spawn_local.z], true);
    }
    
    let spawn_voxel = VoxelCoord::from_ecef(&spawn_ecef);
    println!("✅ Player spawned");
    println!("  Local: ({:.1}, {:.1}, {:.1})", spawn_local.x, spawn_local.y, spawn_local.z);
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", spawn_ecef.x, spawn_ecef.y, spawn_ecef.z);
    println!("  Voxel: ({}, {}, {})\n", spawn_voxel.x, spawn_voxel.y, spawn_voxel.z);
    
    // Update query pipeline with collision meshes (needed for raycasting)
    physics.query_pipeline.update(&physics.colliders);
    
    // DEBUG: Test if raycast works at all
    println!("DEBUG: Testing raycast from player spawn position...");
    let test_ray_origin = point![spawn_local.x, spawn_local.y, spawn_local.z];
    let test_ray_dir = vector![0.0, -1.0, 0.0]; // Straight down
    let test_ray = rapier3d::prelude::Ray::new(test_ray_origin, test_ray_dir);
    let test_max_dist = 50.0;
    
    if let Some((handle, toi)) = physics.query_pipeline.cast_ray(
        &physics.bodies,
        &physics.colliders,
        &test_ray,
        test_max_dist,
        true,
        rapier3d::prelude::QueryFilter::default(),
    ) {
        println!("DEBUG: ✅ Raycast HIT! Distance: {:.2}m, Collider: {:?}", toi, handle);
    } else {
        println!("DEBUG: ❌ Raycast MISS (no collision within {}m)", test_max_dist);
    }
    
    // STEP 8: Setup first-person camera
    println!("STEP 8: Setup first-person camera...");
    // Camera should follow player exactly (we'll update it each frame)
    let aspect = context.size.width as f32 / context.size.height as f32;
    let camera_local = physics.ecef_to_local(&player.camera_position());
    let mut camera = Camera::new(camera_local, aspect);
    camera.yaw = player.camera_yaw;
    camera.pitch = player.camera_pitch;
    println!("✅ Camera positioned at player eyes\n");
    
    // Input state
    let mut move_forward = 0.0f32;
    let mut move_right = 0.0f32;
    let mut jump_pressed = false;
    let mut dig_pressed = false;
    let mut place_pressed = false;
    
    let mut last_frame = Instant::now();
    let mut frame_count = 0;
    let mut fps_timer = Instant::now();
    let mut cursor_grabbed = false;
    let mut mesh_dirty = false;
    
    println!("========================================");
    println!("🎮 DEMO RUNNING");
    println!("Controls:");
    println!("  WASD - Move");
    println!("  Space - Jump");
    println!("  E - Dig");
    println!("  Q - Place");
    println!("  Left Click - Grab mouse");
    println!("  ESC - Quit");
    println!("========================================\n");
    
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    elwt.exit();
                }
                
                WindowEvent::KeyboardInput { event: key_event, .. } => {
                    if key_event.state == ElementState::Pressed {
                        if let PhysicalKey::Code(code) = key_event.physical_key {
                            match code {
                                KeyCode::Escape => elwt.exit(),
                                KeyCode::KeyW => move_forward = 1.0,
                                KeyCode::KeyS => move_forward = -1.0,
                                KeyCode::KeyA => move_right = -1.0,
                                KeyCode::KeyD => move_right = 1.0,
                                KeyCode::Space => jump_pressed = true,
                                KeyCode::KeyE => dig_pressed = true,
                                KeyCode::KeyQ => place_pressed = true,
                                _ => {}
                            }
                        }
                    } else if key_event.state == ElementState::Released {
                        if let PhysicalKey::Code(code) = key_event.physical_key {
                            match code {
                                KeyCode::KeyW | KeyCode::KeyS => move_forward = 0.0,
                                KeyCode::KeyA | KeyCode::KeyD => move_right = 0.0,
                                KeyCode::Space => jump_pressed = false,
                                _ => {}
                            }
                        }
                    }
                }
                
                WindowEvent::MouseInput { button: MouseButton::Left, state: ElementState::Pressed, .. } => {
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
                    let dt = 1.0 / 60.0; // Fixed timestep
                    
                    // Calculate gravity (toward Earth center)
                    let gravity = Vec3::new(
                        -player.position.x as f32,
                        -player.position.y as f32,
                        -player.position.z as f32,
                    ).normalize() * 9.8;
                    
                    // Handle digging
                    if dig_pressed {
                        if let Some(dug) = player.dig_voxel(&mut octree, 5.0) {
                            println!("🔨 Dug voxel at {:?}", dug);
                            mesh_dirty = true;
                        }
                        dig_pressed = false;
                    }
                    
                    // Handle placing
                    if place_pressed {
                        if let Some(placed) = player.place_voxel(&mut octree, MaterialId::STONE, 5.0) {
                            println!("🧱 Placed stone at {:?}", placed);
                            mesh_dirty = true;
                        }
                        place_pressed = false;
                    }
                    
                    // Calculate movement direction in camera space
                    let forward = Vec3::new(
                        camera.yaw.cos(),
                        0.0,
                        camera.yaw.sin()
                    );
                    let right = Vec3::new(
                        (camera.yaw + std::f32::consts::PI / 2.0).cos(),
                        0.0,
                        (camera.yaw + std::f32::consts::PI / 2.0).sin()
                    );
                    let move_dir = forward * move_forward + right * move_right;
                    
                    // Update player physics
                    player.update_ground_detection(&physics);
                    player.apply_movement(&mut physics, move_dir, jump_pressed, dt);
                    player.sync_from_physics(&physics);
                    
                    // Step simulation
                    physics.step(gravity);
                    
                    // Update camera to follow player (first person, in local coordinates)
                    let camera_ecef_pos = player.camera_position();
                    camera.position = physics.ecef_to_local(&camera_ecef_pos);
                    camera.yaw = player.camera_yaw;
                    camera.pitch = player.camera_pitch;
                    
                    // Regenerate mesh if terrain changed
                    if mesh_dirty {
                        println!("🔄 Regenerating mesh...");
                        let new_mesh = extract_octree_mesh(&octree, &origin_voxel, 8);
                        mesh_buffer = MeshBuffer::from_mesh(&context.device, &new_mesh);
                        terrain_collider = update_region_collision(
                            &mut physics,
                            &octree,
                            &origin_voxel,
                            8,
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
                    
                    // Debug output every second
                    frame_count += 1;
                    if fps_timer.elapsed().as_secs() >= 1 {
                        let player_voxel = VoxelCoord::from_ecef(&player.position);
                        println!("FPS: {} | Voxel: ({}, {}, {}) | Ground: {} | Vel: {:.1} m/s",
                            frame_count,
                            player_voxel.x,
                            player_voxel.y,
                            player_voxel.z,
                            player.on_ground,
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
                    camera.process_mouse(delta.0, delta.1);
                    player.camera_yaw = camera.yaw;
                    player.camera_pitch = camera.pitch;
                }
            }
            
            Event::AboutToWait => {
                window.request_redraw();
            }
            
            _ => {}
        }
    }).unwrap();
}
