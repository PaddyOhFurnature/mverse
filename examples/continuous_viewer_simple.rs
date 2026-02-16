//! Continuous Query Viewer
//! Working viewer for continuous world system

use metaverse_core::renderer::{Renderer, pipeline::BasicPipeline};
use metaverse_core::renderer::greedy_mesh::greedy_mesh_block;
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
    camera: Camera,
    continuous_world: Option<ContinuousWorld>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    frame_count: usize,
    fps_update_time: std::time::Instant,
    last_frame_time: std::time::Instant,
    mesh_update_frame: usize,
    keys_pressed: std::collections::HashSet<KeyCode>,
    mouse_captured: bool,
    last_mouse_pos: Option<(f64, f64)>,
}

impl App {
    fn new() -> Self {
        // Start at Kangaroo Point, 20m altitude (ground is ~4m)
        let gps = GpsPos {
            lat_deg: TEST_LAT,
            lon_deg: TEST_LON,
            elevation_m: 20.0,
        };
        let position_ecef = gps_to_ecef(&gps);
        let position = glam::DVec3::new(position_ecef.x, position_ecef.y, position_ecef.z);
        let look_at = glam::DVec3::ZERO; // Look down
        
        let camera = Camera::new(position, look_at);
        
        println!("Camera at Kangaroo Point:");
        println!("  GPS: ({:.6}°, {:.6}°, {:.0}m)", gps.lat_deg, gps.lon_deg, gps.elevation_m);
        println!("  ECEF: ({:.1}, {:.1}, {:.1})", position.x, position.y, position.z);
        
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            camera,
            continuous_world: None,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            frame_count: 0,
            fps_update_time: std::time::Instant::now(),
            last_frame_time: std::time::Instant::now(),
            mesh_update_frame: 0,
            keys_pressed: std::collections::HashSet::new(),
            mouse_captured: false,
            last_mouse_pos: None,
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
        
        println!("[Mesh Update - With Greedy Meshing]");
        
        for (block, lod_level) in &blocks_with_distance {
            // Apply greedy meshing to ALL blocks (no LOD distinction for now)
            // This eliminates "popping" when blocks transition between LOD levels
            
            // Count non-air voxels for stats
            let block_voxels = block.voxels.iter().filter(|&&v| v != AIR).count();
            if block_voxels == 0 { continue; } // Skip empty blocks
            
            voxel_count += block_voxels;
            
            // Use greedy meshing to generate mesh for this block
            let (block_verts, block_inds) = greedy_mesh_block(
                &block.voxels,
                block.ecef_min,
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
        
        println!("  Near blocks: {}, Far blocks: {}", near_blocks, far_blocks);
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
        let (view_proj, camera_offset) = self.camera.view_projection_matrix(aspect);
        let camera_offset_f32 = glam::Vec3::new(
            camera_offset.x as f32,
            camera_offset.y as f32,
            camera_offset.z as f32,
        );
        let origin_transform = glam::Mat4::from_translation(-camera_offset_f32);
        let final_mvp = view_proj * origin_transform;
        pipeline.update_uniforms(&renderer.queue, final_mvp);
        
        // Render to texture and capture
        let clear_color = wgpu::Color { r: 0.53, g: 0.81, b: 0.92, a: 1.0 };
        let num_indices = self.num_indices;
        let vertex_buffer = self.vertex_buffer.as_ref();
        let index_buffer = self.index_buffer.as_ref();
        
        let result = renderer.render_and_capture(clear_color, |render_pass| {
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
            
            println!("\nInitializing continuous world (this takes ~5 seconds)...");
            println!("  Pre-generating 10,404 terrain blocks...");
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
            println!("\n✓ Ready! Use WASD to move, mouse to look around.\n");
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
                        let sensitivity = 0.002;
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
                
                // Update mesh every 30 frames
                if self.frame_count == 0 || (self.frame_count - self.mesh_update_frame) >= 30 {
                    self.update_mesh();
                    self.mesh_update_frame = self.frame_count;
                }
                
                // Render
                if let (Some(window), Some(renderer), Some(pipeline)) = 
                    (&self.window, &mut self.renderer, &self.pipeline) {
                    
                    // Update camera uniforms
                    let aspect = renderer.size.width as f32 / renderer.size.height as f32;
                    let (view_proj, camera_offset) = self.camera.view_projection_matrix(aspect);
                    let camera_offset_f32 = glam::Vec3::new(
                        camera_offset.x as f32,
                        camera_offset.y as f32,
                        camera_offset.z as f32,
                    );
                    let origin_transform = glam::Mat4::from_translation(-camera_offset_f32);
                    let final_mvp = view_proj * origin_transform;
                    pipeline.update_uniforms(&renderer.queue, final_mvp);
                    
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
                    
                    // FPS
                    self.frame_count += 1;
                    if now.duration_since(self.fps_update_time).as_secs_f32() >= 1.0 {
                        let fps = self.frame_count as f32 / now.duration_since(self.fps_update_time).as_secs_f32();
                        let pos = self.camera.position;
                        let gps = ecef_to_gps(&EcefPos { x: pos.x, y: pos.y, z: pos.z });
                        window.set_title(&format!(
                            "Continuous Viewer - {:.0} FPS | ({:.6}°, {:.6}°) {:.0}m",
                            fps, gps.lat_deg, gps.lon_deg, gps.elevation_m
                        ));
                        self.frame_count = 0;
                        self.fps_update_time = now;
                    }
                    
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    println!("=== Continuous Query Viewer ===");
    println!("Location: Kangaroo Point, Brisbane");
    println!("\nControls:");
    println!("  WASD - Move");
    println!("  Space/Shift - Up/Down");
    println!("  Left Click - Capture mouse");
    println!("  R - Reload mesh");
    println!("  F5 - Screenshot");
    println!("  ESC - Exit\n");
    
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    event_loop.run_app(&mut app).unwrap();
}
