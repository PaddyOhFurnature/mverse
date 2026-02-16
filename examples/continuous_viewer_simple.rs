//! Continuous Query Viewer
//! Working viewer for continuous world system

use metaverse_core::renderer::{Renderer, pipeline::BasicPipeline};
use metaverse_core::renderer::greedy_mesh::greedy_mesh_block;
use metaverse_core::renderer::frustum::Frustum;
use metaverse_core::renderer::skybox::SkyboxPipeline;
use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::renderer::camera::Camera;
use metaverse_core::renderer::pipeline::Vertex;
use metaverse_core::coordinates::{gps_to_ecef, ecef_to_gps, GpsPos, EcefPos};
use metaverse_core::svo::{AIR, MaterialId, STONE, DIRT, GRASS, WATER, CONCRETE, ASPHALT};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::*;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};
use wgpu;
use glam::Vec3;

// Kangaroo Point, Brisbane
const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

/// Map material ID to RGB color
fn material_color(material: MaterialId) -> [f32; 4] {
    match material {
        AIR => [0.0, 0.0, 0.0, 0.0],           // Transparent (shouldn't render)
        STONE => [0.5, 0.5, 0.5, 1.0],         // Gray stone
        DIRT => [0.6, 0.4, 0.2, 1.0],          // Brown dirt
        CONCRETE => [0.7, 0.7, 0.7, 1.0],      // Light gray concrete
        WATER => [0.2, 0.4, 0.8, 1.0],         // Blue water
        GRASS => [0.2, 0.8, 0.2, 1.0],         // Green grass
        ASPHALT => [0.3, 0.3, 0.3, 1.0],       // Dark gray asphalt
        MaterialId(9) => [0.9, 0.8, 0.6, 1.0], // SAND - Tan
        MaterialId(4) => [0.6, 0.3, 0.1, 1.0], // WOOD - Brown
        MaterialId(10) => [0.7, 0.3, 0.2, 1.0],// BRICK - Red-brown
        _ => [0.8, 0.2, 0.8, 1.0],             // Magenta for unknown materials
    }
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pipeline: Option<BasicPipeline>,
    skybox: Option<SkyboxPipeline>,
    camera: Camera,
    continuous_world: Option<ContinuousWorld>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    frame_count: usize,      // Frames since last FPS update (resets every second)
    total_frames: usize,     // Total frames ever (for mesh updates)
    fps_update_time: std::time::Instant,
    last_frame_time: std::time::Instant,
    last_mesh_update: usize, // Total frame count when mesh was last updated
    keys_pressed: std::collections::HashSet<KeyCode>,
    mouse_captured: bool,
    last_mouse_pos: Option<(f64, f64)>,
    screenshot_mode: bool, // Take one screenshot and exit
}

impl App {
    fn new(gps: GpsPos, pitch_deg: f64, screenshot_mode: bool) -> Self {
        let position_ecef = gps_to_ecef(&gps);
        let position = glam::DVec3::new(position_ecef.x, position_ecef.y, position_ecef.z);
        let look_at = glam::DVec3::ZERO; // Look down
        
        let mut camera = Camera::new(position, look_at);
        
        // Apply pitch if specified
        if pitch_deg != 0.0 {
            camera.rotate(pitch_deg.to_radians(), 0.0);
        }
        
        println!("Camera at:");
        println!("  GPS: ({:.6}°, {:.6}°, {:.0}m)", gps.lat_deg, gps.lon_deg, gps.elevation_m);
        println!("  ECEF: ({:.1}, {:.1}, {:.1})", position.x, position.y, position.z);
        println!("  Pitch: {:.1}°", pitch_deg);
        
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            skybox: None,
            camera,
            continuous_world: None,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            frame_count: 0,
            total_frames: 0,
            fps_update_time: std::time::Instant::now(),
            last_frame_time: std::time::Instant::now(),
            last_mesh_update: 0,
            keys_pressed: std::collections::HashSet::new(),
            mouse_captured: false,
            last_mouse_pos: None,
            screenshot_mode,
        }
    }
    
    fn handle_input(&mut self, delta_time: f64) {
        let mut forward = 0.0;
        let mut right = 0.0;
        let mut up = 0.0;
        
        if self.keys_pressed.contains(&KeyCode::KeyW) { forward += 1.0; }
        if self.keys_pressed.contains(&KeyCode::KeyS) { forward -= 1.0; }
        if self.keys_pressed.contains(&KeyCode::KeyD) { right += 1.0; }
        if self.keys_pressed.contains(&KeyCode::KeyA) { right -= 1.0; }
        if self.keys_pressed.contains(&KeyCode::Space) { up += 1.0; }
        if self.keys_pressed.contains(&KeyCode::ShiftLeft) { up -= 1.0; }
        
        self.camera.move_relative(forward, right, up, delta_time);
    }
    
    fn update_mesh(&mut self) {
        let Some(renderer) = &self.renderer else { return };
        let Some(world) = &mut self.continuous_world else { return };
        
        println!("\n[Mesh Update]");
        
        // Query 100m radius with LOD
        let cam_pos = [
            self.camera.position.x,
            self.camera.position.y,
            self.camera.position.z,
        ];
        let blocks_with_distance = world.query_lod(cam_pos, 100.0);
        
        println!("  Queried {} blocks with LOD in 100m radius", blocks_with_distance.len());
        
        // Render with block-level LOD + Greedy Meshing
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut near_blocks = 0;
        let mut far_blocks = 0;
        let mut voxel_count = 0;
        let mut culled_blocks = 0;
        
        // Build frustum for culling
        let renderer = self.renderer.as_ref().unwrap();
        let aspect = renderer.size.width as f32 / renderer.size.height as f32;
        let (view_proj, _offset) = self.camera.view_projection_matrix(aspect);
        let frustum = Frustum::from_view_projection(&view_proj);
        
        println!("[Mesh Update - With Greedy Meshing + Frustum Culling]");
        
        for (block, lod_level) in &blocks_with_distance {
            // Count non-air voxels for stats
            let block_voxels = block.voxels.iter().filter(|&&v| v != AIR).count();
            if block_voxels == 0 { continue; } // Skip empty blocks
            
            // CRITICAL FIX: Render relative to camera (f64 precision)
            // Convert ECEF block offset to camera-relative coordinates BEFORE f32 conversion
            let block_relative_to_cam = [
                block.ecef_min[0] - cam_pos[0],
                block.ecef_min[1] - cam_pos[1],
                block.ecef_min[2] - cam_pos[2],
            ];
            
            // Frustum culling: check if block AABB is visible
            let block_min = Vec3::new(
                block_relative_to_cam[0] as f32,
                block_relative_to_cam[1] as f32,
                block_relative_to_cam[2] as f32,
            );
            let block_max = block_min + Vec3::splat(8.0); // 8m block size
            
            if !frustum.intersects_aabb(block_min, block_max) {
                culled_blocks += 1;
                continue; // Skip off-screen blocks
            }
            
            voxel_count += block_voxels;
            
            // Use greedy meshing with camera-relative offset
            let (block_verts, block_inds) = greedy_mesh_block(
                &block.voxels,
                block_relative_to_cam, // NOW small numbers (~0-100m), safe for f32
            );
            
            // Offset indices to account for existing vertices
            let base_idx = vertices.len() as u32;
            vertices.extend(block_verts);
            indices.extend(block_inds.iter().map(|idx| idx + base_idx));
            
            // Track LOD for stats
            if *lod_level <= 1 {
                near_blocks += 1;
            } else {
                far_blocks += 1;
            }
        }
        
        println!("  Near blocks: {}, Far blocks: {}, Culled: {}", near_blocks, far_blocks, culled_blocks);
        println!("  {} primitives → {} vertices, {} indices", voxel_count, vertices.len(), indices.len());
        
        if vertices.is_empty() {
            println!("  No geometry to render!");
            return;
        }
        
        // Update GPU buffers
        use wgpu::util::DeviceExt;
        
        let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Continuous World Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        
        let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Continuous World Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        
        self.vertex_buffer = Some(vertex_buffer);
        self.index_buffer = Some(index_buffer);
        self.num_indices = indices.len() as u32;
        
        println!("  ✓ Mesh updated");
    }
    
    fn capture_screenshot(&mut self) {
        let Some(renderer) = &mut self.renderer else { return };
        let Some(pipeline) = &self.pipeline else { return };
        
        // Update camera uniforms
        let aspect = renderer.size.width as f32 / renderer.size.height as f32;
        let (view_proj, _camera_offset) = self.camera.view_projection_matrix(aspect);
        
        // NOTE: Vertices are already camera-relative (subtracted camera pos in update_mesh)
        // So we don't need origin_transform here - just use view_proj directly
        pipeline.update_uniforms(&renderer.queue, view_proj);
        
        // Render to texture and capture
        // Use sky blue clear color (skybox gradient causes lifetime issues in closure)
        let clear_color = wgpu::Color { r: 0.53, g: 0.71, b: 0.9, a: 1.0 }; // Light sky blue
        let num_indices = self.num_indices;
        let vertex_buffer = self.vertex_buffer.as_ref();
        let index_buffer = self.index_buffer.as_ref();
        
        let result = renderer.render_and_capture(clear_color, |render_pass| {
            // Render terrain
            if let (Some(vb), Some(ib)) = (vertex_buffer, index_buffer) {
                if num_indices > 0 {
                    render_pass.set_pipeline(&pipeline.pipeline);
                    render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, vb.slice(..));
                    render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..num_indices, 0, 0..1);
                }
            }
        });
        
        match result {
            Ok((pixels, width, height)) => {
                // Save to PNG
                let pos = self.camera.position;
                let gps = ecef_to_gps(&EcefPos { x: pos.x, y: pos.y, z: pos.z });
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let filename = format!(
                    "screenshot/continuous_{}_{:.6}_{:.6}.png",
                    timestamp,
                    gps.lat_deg,
                    gps.lon_deg
                );
                
                if let Err(e) = image::save_buffer(
                    &filename,
                    &pixels,
                    width,
                    height,
                    image::ColorType::Rgba8
                ) {
                    eprintln!("  ✗ Failed to save: {}", e);
                } else {
                    println!("  ✓ Screenshot saved: {}", filename);
                    
                    // If in screenshot mode, exit immediately
                    if self.screenshot_mode {
                        println!("✓ Screenshot mode complete - exiting");
                        std::process::exit(0);
                    }
                }
            }
            Err(e) => {
                eprintln!("  ✗ Capture failed: {}", e);
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = Arc::new(
                event_loop.create_window(
                    WindowAttributes::default()
                        .with_title("Continuous Query Viewer")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
                ).unwrap()
            );
            
            println!("\nInitializing renderer...");
            let renderer = pollster::block_on(Renderer::new(window.clone()));
            println!("✓ Renderer ready");
            
            let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
            println!("✓ Pipeline ready");
            
            println!("\nInitializing continuous world...");
            let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
            let center_ecef = gps_to_ecef(&gps_center);
            let center = [center_ecef.x, center_ecef.y, center_ecef.z];
            
            let world = match ContinuousWorld::new(center, 100.0) {
                Ok(w) => {
                    println!("✓ World created");
                    w
                }
                Err(e) => {
                    eprintln!("✗ Failed to create world: {}", e);
                    return;
                }
            };
            
            self.window = Some(window);
            self.renderer = Some(renderer);
            self.pipeline = Some(pipeline);
            self.continuous_world = Some(world);
            
            println!("\nGenerating initial mesh (50m radius)...");
            self.update_mesh();
            // Don't set last_mesh_update - let first frame set it
            println!("\n✓ Ready!");
            
            // If screenshot mode, take screenshot immediately
            if self.screenshot_mode {
                println!("\n[Screenshot Mode] Taking screenshot and exiting...");
                self.capture_screenshot();
            } else {
                println!("\nUse WASD to move, mouse to look around.\n");
            }
        }
    }
    
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested | WindowEvent::KeyboardInput {
                event: KeyEvent {
                    physical_key: PhysicalKey::Code(KeyCode::Escape),
                    state: ElementState::Pressed,
                    ..
                },
                ..
            } => event_loop.exit(),
            
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    physical_key: PhysicalKey::Code(key_code),
                    state,
                    ..
                },
                ..
            } => {
                match state {
                    ElementState::Pressed => {
                        self.keys_pressed.insert(key_code);
                        if key_code == KeyCode::KeyR {
                            println!("\n[R] Reloading mesh...");
                            self.update_mesh();
                        } else if key_code == KeyCode::F5 {
                            println!("\n[F5] Capturing screenshot...");
                            self.capture_screenshot();
                        }
                    }
                    ElementState::Released => { self.keys_pressed.remove(&key_code); }
                }
            }
            
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                self.mouse_captured = !self.mouse_captured;
                if let Some(window) = &self.window {
                    if self.mouse_captured {
                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                        window.set_cursor_visible(false);
                    } else {
                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                        window.set_cursor_visible(true);
                    }
                }
            }
            
            WindowEvent::CursorMoved { position, .. } => {
                if self.mouse_captured {
                    if let Some(last_pos) = self.last_mouse_pos {
                        let dx = position.x - last_pos.0;
                        let dy = position.y - last_pos.1;
                        let sensitivity = 0.004; // Doubled from 0.002
                        self.camera.rotate(-dy * sensitivity, -dx * sensitivity);
                    }
                    self.last_mouse_pos = Some((position.x, position.y));
                } else {
                    self.last_mouse_pos = None;
                }
            }
            
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size);
                }
            }
            
            WindowEvent::RedrawRequested => {
                // Handle input
                let now = std::time::Instant::now();
                let delta_time = now.duration_since(self.last_frame_time).as_secs_f64();
                self.last_frame_time = now;
                self.handle_input(delta_time);
                
                // Update mesh every 60 frames (1 second at 60fps, 2 seconds at 30fps)
                // Only after first frame sets the baseline
                if self.last_mesh_update == 0 {
                    self.last_mesh_update = self.total_frames; // Set baseline on first frame
                } else if self.total_frames >= self.last_mesh_update + 60 {
                    self.update_mesh();
                    self.last_mesh_update = self.total_frames;
                }
                
                // Render
                if let (Some(window), Some(renderer), Some(pipeline)) = 
                    (&self.window, &mut self.renderer, &self.pipeline) {
                    
                    // Update camera uniforms
                    let aspect = renderer.size.width as f32 / renderer.size.height as f32;
                    let (view_proj, _camera_offset) = self.camera.view_projection_matrix(aspect);
                    
                    // NOTE: Vertices are already camera-relative (subtracted in update_mesh)
                    pipeline.update_uniforms(&renderer.queue, view_proj);
                    
                    // Render
                    let clear_color = wgpu::Color { r: 0.53, g: 0.81, b: 0.92, a: 1.0 };
                    let num_indices = self.num_indices;
                    let vertex_buffer = self.vertex_buffer.as_ref();
                    let index_buffer = self.index_buffer.as_ref();
                    
                    let result = renderer.render(clear_color, |render_pass| {
                        if let (Some(vb), Some(ib)) = (vertex_buffer, index_buffer) {
                            if num_indices > 0 {
                                render_pass.set_pipeline(&pipeline.pipeline);
                                render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                                render_pass.set_vertex_buffer(0, vb.slice(..));
                                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                                render_pass.draw_indexed(0..num_indices, 0, 0..1);
                            }
                        }
                    });
                    
                    match result {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => renderer.resize(renderer.size),
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => eprintln!("Render error: {:?}", e),
                    }
                    
                    // FPS counter and frame tracking
                    self.frame_count += 1;
                    self.total_frames += 1;
                    
                    if now.duration_since(self.fps_update_time).as_secs_f32() >= 1.0 {
                        let fps = self.frame_count as f32 / now.duration_since(self.fps_update_time).as_secs_f32();
                        let pos = self.camera.position;
                        let gps = ecef_to_gps(&EcefPos { x: pos.x, y: pos.y, z: pos.z });
                        window.set_title(&format!(
                            "Continuous Viewer - {:.1} FPS | ({:.6}°, {:.6}°) {:.0}m",
                            fps, gps.lat_deg, gps.lon_deg, gps.elevation_m
                        ));
                        self.frame_count = 0; // Reset for next FPS calculation
                        self.fps_update_time = now;
                    }
                    
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
    
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Request continuous rendering
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn parse_args() -> (GpsPos, f64, bool) {
    let args: Vec<String> = std::env::args().collect();
    let mut lat = TEST_LAT;
    let mut lon = TEST_LON;
    let mut alt = 20.0;
    let mut pitch = -20.0;
    let mut screenshot = false;
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--lat" if i + 1 < args.len() => {
                lat = args[i + 1].parse().unwrap_or(TEST_LAT);
                i += 2;
            }
            "--lon" if i + 1 < args.len() => {
                lon = args[i + 1].parse().unwrap_or(TEST_LON);
                i += 2;
            }
            "--alt" if i + 1 < args.len() => {
                alt = args[i + 1].parse().unwrap_or(20.0);
                i += 2;
            }
            "--pitch" if i + 1 < args.len() => {
                pitch = args[i + 1].parse().unwrap_or(-20.0);
                i += 2;
            }
            "--screenshot" => {
                screenshot = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    
    (GpsPos { lat_deg: lat, lon_deg: lon, elevation_m: alt }, pitch, screenshot)
}

fn main() {
    let (gps, pitch, screenshot_mode) = parse_args();
    
    if screenshot_mode {
        println!("=== Screenshot Mode ===");
    } else {
        println!("=== Continuous Query Viewer ===");
        println!("\nControls:");
        println!("  WASD - Move");
        println!("  Space/Shift - Up/Down");
        println!("  Left Click - Capture mouse");
        println!("  R - Reload mesh");
        println!("  F5 - Screenshot");
        println!("  ESC - Exit\n");
    }
    
    println!("Location: ({:.6}°, {:.6}°, {:.1}m)", gps.lat_deg, gps.lon_deg, gps.elevation_m);
    
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new(gps, pitch, screenshot_mode);
    event_loop.run_app(&mut app).unwrap();
}
