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
//!
//! # Usage
//!
//! **Terminal 1:**
//! ```bash
//! cargo run --release --example phase1_multiplayer
//! ```
//!
//! **Terminal 2:**
//! ```bash
//! cargo run --release --example phase1_multiplayer
//! ```
//!
//! Both instances will auto-discover each other via mDNS within 1-2 seconds.
//! Move around in one window, see your player move in the other window.
//! Dig/place blocks - changes appear in both clients.
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
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource, OpenTopographySource},
    identity::Identity,
    marching_cubes::extract_octree_mesh,
    materials::MaterialId,
    mesh::{Mesh, Vertex},
    messages::{Material, MovementMode},
    multiplayer::MultiplayerSystem,
    physics::{PhysicsWorld, Player},
    remote_render::{create_remote_player_capsule, remote_player_transform, short_peer_id},
    renderer::{Camera, MeshBuffer, RenderContext, RenderPipeline},
    terrain::TerrainGenerator,
    user_content::UserContentLayer,
    voxel::{Octree, VoxelCoord},
};
use glam::{Mat4, Vec3};
use rapier3d::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    event::*,
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlayerModeLocal {
    Walk,  // Physics-based, can walk/jump
    Fly,   // Free movement, no gravity
}

fn main() {
    env_logger::init();
    
    println!("=== Phase 1 Multiplayer Demo ===");
    println!();
    println!("🌐 P2P NETWORKING ENABLED");
    println!("   - Auto-discovery via mDNS (localhost)");
    println!("   - Player state sync @ 20 Hz");
    println!("   - Voxel operations with CRDT");
    println!("   - Ed25519 signatures");
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
    
    // Check for --temp-identity flag for testing multiple instances
    let identity = if std::env::args().any(|arg| arg == "--temp-identity") {
        println!("   Using temporary identity (not saved)");
        Identity::generate()
    } else {
        Identity::load_or_create()
            .expect("Failed to create identity")
    };
    
    println!("   PeerId: {}", short_peer_id(&identity.peer_id()));
    println!("   Key: ~/.metaverse/identity.key");
    
    println!("\n🌐 Starting P2P network node...");
    let mut multiplayer = MultiplayerSystem::new_with_runtime(identity)
        .expect("Failed to create multiplayer system");
    
    // Start listening on random port
    multiplayer.listen_on("/ip4/0.0.0.0/tcp/0")
        .expect("Failed to start listening");
    
    println!("   Listening for connections...");
    println!("   mDNS discovery active (auto-connect on LAN)");
    println!("   PeerId: {}", multiplayer.peer_id());
    println!("\n⏳ Waiting for peers to connect...");
    println!("   (Watch for \"Peer discovered\" and \"Peer connected\" messages)");
    println!("   Note: Publishing will fail until at least one peer connects - this is normal!\n");
    println!();
    
    // Create window
    let event_loop = EventLoop::new().unwrap();
    let window = event_loop
        .create_window(
            winit::window::WindowAttributes::default()
                .with_title("Phase 1 Multiplayer - Metaverse Core")
                .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080))
        )
        .unwrap();
    
    let window = Arc::new(window);
    
    // Initialize renderer
    println!("🎨 Initializing renderer...");
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let mut pipeline = RenderPipeline::new(&context);
    
    // Generate terrain with real SRTM data
    println!("🗺️  Generating terrain (100m × 100m Brisbane)...");
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
    
    println!("   Terrain generated in {:.2}s", start.elapsed().as_secs_f32());
    
    // Convert GPS origin to voxel coordinates  
    let origin_ecef = origin_gps.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    
    println!("   Origin GPS: ({:.6}, {:.6}, {:.1}m)", origin_gps.lat, origin_gps.lon, origin_gps.alt);
    println!("   Origin voxel: {:?}", origin_voxel);
    
    // Extract initial mesh
    println!("🔺 Extracting mesh...");
    let mesh_start = Instant::now();
    let mesh = extract_octree_mesh(&octree, &origin_voxel, 7); // 128 voxel region
    println!("   Mesh extracted in {:.2}s ({} vertices)", 
        mesh_start.elapsed().as_secs_f32(), 
        mesh.vertices.len()
    );
    
    // Find ground level at origin
    let mut ground_y = f32::MIN;
    for v in &mesh.vertices {
        if v.position.x.abs() < 5.0 && v.position.z.abs() < 5.0 {
            ground_y = ground_y.max(v.position.y);
        }
    }
    if ground_y < -500.0 {
        ground_y = 0.0;
    }
    
    println!("   Ground level at spawn: {:.1}m", ground_y);
    
    // Upload mesh to GPU
    let mut mesh_buffer = MeshBuffer::from_mesh(&context.device, &mesh);
    
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
    
    // Initialize physics world with FloatingOrigin at origin
    let origin_voxel_ecef = origin_voxel.to_ecef();
    let mut physics = PhysicsWorld::with_origin(origin_voxel_ecef);
    
    // Generate collision mesh for terrain
    println!("💥 Generating physics collision...");
    let mut terrain_collider = metaverse_core::physics::update_region_collision(
        &mut physics,
        &octree,
        &origin_voxel,
        7,
        None,
    );
    
    // Spawn player 3m above ground
    let mut player = Player::new(&mut physics, origin_gps, 0.0);
    let spawn_local = Vec3::new(0.0, ground_y + 3.0, 0.0);
    let spawn_ecef = physics.local_to_ecef(spawn_local);
    player.position = spawn_ecef;
    
    if let Some(body) = physics.bodies.get_mut(player.body_handle) {
        body.set_translation(vector![spawn_local.x, spawn_local.y, spawn_local.z], true);
    }
    
    let player_local = physics.ecef_to_local(&player.position);
    println!("✅ Player spawned at local: ({:.1}, {:.1}, {:.1})", 
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
    
    let mut cursor_grabbed = false;
    let mut mesh_dirty = false;
    
    // Track local voxel operations for CRDT merge
    let mut local_voxel_ops: HashMap<VoxelCoord, metaverse_core::messages::VoxelOperation> = HashMap::new();
    
    // User content layer - separates edits from base terrain
    let mut user_content = UserContentLayer::new();
    
    println!("\n🎮 Demo running!");
    println!("   Waiting for peers to connect...");
    println!("   (Run another instance to test P2P)\n");
    
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    println!("\n👋 Shutting down...");
                    elwt.exit();
                }
                
                WindowEvent::KeyboardInput { event, .. } => {
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
                    let dt = 1.0 / 60.0;
                    
                    // Update multiplayer system (polls network, interpolates remote players)
                    multiplayer.update(dt);
                    
                    // Handle chat
                    if chat_pressed {
                        let _ = multiplayer.send_chat("Hello from P2P!".to_string());
                        println!("💬 Sent chat message");
                        chat_pressed = false;
                    }
                    
                    // Handle digging
                    if dig_pressed {
                        if let Some(dug) = player.dig_voxel(&physics, &mut octree, 10.0) {
                            println!("⛏️  Dug voxel at {:?}", dug);
                            
                            // Broadcast voxel operation
                            match multiplayer.broadcast_voxel_operation(dug, Material::Air) {
                                Ok(op) => {
                                    // Track locally for CRDT merge
                                    local_voxel_ops.insert(dug, op.clone());
                                    
                                    // Log to user content layer
                                    if let Err(e) = user_content.apply_operation(op, &local_voxel_ops) {
                                        eprintln!("Failed to log local dig operation: {:?}", e);
                                    }
                                }
                                Err(e) => eprintln!("Failed to broadcast dig: {}", e),
                            }
                            
                            mesh_dirty = true;
                        }
                        dig_pressed = false;
                    }
                    
                    // Handle placing
                    if place_pressed {
                        if let Some(placed) = player.place_voxel(&physics, &mut octree, MaterialId::STONE, 10.0) {
                            println!("🧱 Placed voxel at {:?}", placed);
                            
                            // Broadcast voxel operation
                            match multiplayer.broadcast_voxel_operation(placed, Material::Stone) {
                                Ok(op) => {
                                    // Track locally for CRDT merge
                                    local_voxel_ops.insert(placed, op.clone());
                                    
                                    // Log to user content layer
                                    if let Err(e) = user_content.apply_operation(op, &local_voxel_ops) {
                                        eprintln!("Failed to log local place operation: {:?}", e);
                                    }
                                }
                                Err(e) => eprintln!("Failed to broadcast place: {}", e),
                            }
                            
                            mesh_dirty = true;
                        }
                        place_pressed = false;
                    }
                    
                    // Process any received voxel operations
                    let pending_ops = multiplayer.take_pending_operations();
                    if !pending_ops.is_empty() {
                        println!("📦 Processing {} received voxel operations", pending_ops.len());
                        for op in pending_ops {
                            // Apply to user content layer (handles CRDT merge)
                            match user_content.apply_operation(op.clone(), &local_voxel_ops) {
                                Ok(true) => {
                                    // Operation accepted - apply to octree
                                    let material_id = op.material.to_material_id();
                                    octree.set_voxel(op.coord, material_id);
                                    mesh_dirty = true;
                                    println!("✅ Applied remote voxel operation at {:?}", op.coord);
                                }
                                Ok(false) => {
                                    println!("⚠️  Rejected remote voxel operation (CRDT conflict - local wins)");
                                }
                                Err(e) => {
                                    println!("❌ Invalid remote voxel operation: {:?}", e);
                                }
                            }
                        }
                        
                        // Log current operation count
                        println!("📊 Total operations in log: {}", user_content.op_count());
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
                    
                    // Debug: Print local position every 60 frames
                    if frame_count % 60 == 0 {
                        println!("📤 Broadcasting state: ECEF=({:.1}, {:.1}, {:.1}), Local=({:.1}, {:.1}, {:.1})",
                            player.position.x, player.position.y, player.position.z,
                            player_local_pos.x, player_local_pos.y, player_local_pos.z);
                    }
                    
                    jump_pressed = false;
                    
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
                    
                    // Regenerate mesh if terrain changed
                    if mesh_dirty {
                        println!("🔄 Regenerating mesh and collision...");
                        let new_mesh = extract_octree_mesh(&octree, &origin_voxel, 7);
                        mesh_buffer = MeshBuffer::from_mesh(&context.device, &new_mesh);
                        
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
                                
                                // Render local player hitbox
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
                            frame.present();
                        }
                        Err(e) => eprintln!("Surface error: {:?}", e),
                    }
                    
                    // FPS counter and stats
                    frame_count += 1;
                    if fps_timer.elapsed().as_secs() >= 1 {
                        let stats = multiplayer.stats();
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
    }).unwrap();
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

/// Take screenshot (simplified - just print message for now)
fn take_screenshot(
    _context: &RenderContext,
    _pipeline: &mut RenderPipeline,
    _camera: &mut Camera,
    player: &Player,
    physics: &PhysicsWorld,
    _mesh_buffer: &MeshBuffer,
    _hitbox_buffer: &MeshBuffer,
    _player_model_bind_group: &wgpu::BindGroup,
) {
    use std::fs;
    
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let player_local = physics.ecef_to_local(&player.position);
    
    let filename = format!("screenshot/mp_{}_{:.0}_{:.0}_{:.0}_y{:.1}_p{:.1}.png",
        timestamp,
        player_local.x,
        player_local.y,
        player_local.z,
        player.camera_yaw,
        player.camera_pitch
    );
    
    fs::create_dir_all("screenshot").ok();
    
    println!("📸 Screenshot path: {}", filename);
}
