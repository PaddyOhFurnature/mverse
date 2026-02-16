//! Continuous Query Viewer - Validation Tool
//!
//! Simple viewer to validate the continuous query system works correctly.
//! Renders the Kangaroo Point test area using ContinuousWorld API.

use metaverse_core::renderer::{
    camera::Camera, 
    pipeline::BasicPipeline,
    Renderer
};
use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::*;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};
use wgpu::util::DeviceExt;

// Test location: Kangaroo Point, Brisbane (-27.479769°, 153.033586°)
const KANGAROO_POINT: [f64; 3] = [-5046877.97, 2567787.42, -2925481.59];

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
    
    // Input state
    keys_pressed: std::collections::HashSet<KeyCode>,
    mouse_captured: bool,
    last_mouse_pos: Option<(f64, f64)>,
}

impl App {
    fn new() -> Self {
        // Start camera at Kangaroo Point, 100m altitude looking down
        use crate::coordinates::{gps_to_ecef, GpsPos};
        
        let gps = GpsPos {
            lat_deg: -27.479769,
            lon_deg: 153.033586,
            elevation_m: 100.0,
        };
        let position_ecef = gps_to_ecef(&gps);
        let position = glam::DVec3::new(position_ecef.x, position_ecef.y, position_ecef.z);
        
        // Look toward Earth center (down)
        let look_at = glam::DVec3::ZERO;
        
        let camera = Camera::new(position, look_at);
        
        println!("Camera initialized at Kangaroo Point");
        println!("  GPS: ({:.6}°, {:.6}°, {:.2}m)", gps.lat_deg, gps.lon_deg, gps.elevation_m);
        println!("  ECEF: ({:.1}, {:.1}, {:.1})", position.x, position.y, position.z);
        println!("  Looking straight down at ground");
        
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            camera,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            continuous_world: None,
            frame_count: 0,
            fps_update_time: std::time::Instant::now(),
            last_frame_time: std::time::Instant::now(),
            keys_pressed: std::collections::HashSet::new(),
            mouse_captured: false,
            last_mouse_pos: None,
        }
    }
    
    fn handle_input(&mut self, delta_time: f64) {
        // Movement: WASD + QE for up/down
        let mut forward = 0.0;
        let mut right = 0.0;
        let mut up = 0.0;
        
        if self.keys_pressed.contains(&KeyCode::KeyW) {
            forward += 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyS) {
            forward -= 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyD) {
            right += 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyA) {
            right -= 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyE) || self.keys_pressed.contains(&KeyCode::Space) {
            up += 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyQ) {
            up -= 1.0;
        }
        
        // Speed: 50 m/s normal, 500 m/s with shift
        let speed_mult = if self.keys_pressed.contains(&KeyCode::ShiftLeft) {
            10.0
        } else {
            1.0
        };
        
        self.camera.speed_multiplier = speed_mult;
        self.camera.move_relative(forward, right, up, delta_time);
    }
    
    fn update_world_mesh(&mut self) {
        let Some(renderer) = &self.renderer else { return };
        let Some(world) = &mut self.continuous_world else { return };
        
        println!("\n=== Updating World Mesh ===");
        
        // Query blocks around camera (50m radius for initial test)
        let camera_pos = [
            self.camera.position.x as f64,
            self.camera.position.y as f64,
            self.camera.position.z as f64,
        ];
        
        let query_radius = 50.0;
        let query = AABB::from_center(camera_pos, query_radius);
        
        println!("Camera position: ({:.1}, {:.1}, {:.1})", camera_pos[0], camera_pos[1], camera_pos[2]);
        println!("Query radius: {}m", query_radius);
        
        // Query continuous world for blocks
        let blocks = world.query_range(query);
        println!("Got {} blocks from continuous query", blocks.len());
        
        if blocks.is_empty() {
            println!("WARNING: No blocks returned!");
            return;
        }
        
        // Convert blocks to meshes
        let mut all_vertices = Vec::new();
        let mut all_indices = Vec::new();
        
        let material_colors = MaterialColors::default();
        
        for (i, block) in blocks.iter().enumerate() {
            // Convert VoxelBlock SVO to meshes
            let meshes = block.svo.extract_meshes();
            
            if meshes.is_empty() {
                continue;
            }
            
            println!("Block {}: {} meshes", i, meshes.len());
            
            // Convert to colored vertices
            let (vertices, indices) = svo_meshes_to_colored_vertices(&meshes, &material_colors);
            
            if vertices.is_empty() || indices.is_empty() {
                continue;
            }
            
            println!("  {} vertices, {} indices", vertices.len(), indices.len());
            
            // Append to combined buffers (adjust indices for offset)
            let vertex_offset = all_vertices.len() as u32;
            all_vertices.extend(vertices);
            all_indices.extend(indices.iter().map(|i| i + vertex_offset));
        }
        
        if all_vertices.is_empty() || all_indices.is_empty() {
            println!("No vertices/indices generated from {} blocks", blocks.len());
            return;
        }
        
        println!("Total: {} vertices, {} indices", all_vertices.len(), all_indices.len());
        
        // Update GPU buffers
        let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Continuous World Vertex Buffer"),
            contents: bytemuck::cast_slice(&all_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        
        let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Continuous World Index Buffer"),
            contents: bytemuck::cast_slice(&all_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        
        self.vertex_buffer = Some(vertex_buffer);
        self.index_buffer = Some(index_buffer);
        self.num_indices = all_indices.len() as u32;
        
        println!("✓ Mesh updated successfully\n");
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = Arc::new(
                event_loop
                    .create_window(
                        WindowAttributes::default()
                            .with_title("Continuous Query Viewer - Validation")
                            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                    )
                    .unwrap(),
            );

            // Create renderer
            println!("Creating renderer...");
            let renderer = pollster::block_on(Renderer::new(window.clone()));
            println!("✓ Renderer created");
            
            // Create pipeline
            println!("Creating pipeline...");
            let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
            println!("✓ Pipeline created");
            
            // Initialize continuous world
            println!("Initializing continuous world at Kangaroo Point...");
            let world = match ContinuousWorld::new(KANGAROO_POINT, 100.0) {
                Ok(w) => {
                    println!("✓ Continuous world created (100m radius)");
                    w
                }
                Err(e) => {
                    eprintln!("✗ Failed to create continuous world: {}", e);
                    return;
                }
            };
            
            self.window = Some(window);
            self.renderer = Some(renderer);
            self.pipeline = Some(pipeline);
            self.continuous_world = Some(world);
            
            // Generate initial mesh
            println!("\nGenerating initial mesh...");
            self.update_world_mesh();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            
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
                        
                        // Escape to exit
                        if key_code == KeyCode::Escape {
                            event_loop.exit();
                        }
                        
                        // R to reload mesh
                        if key_code == KeyCode::KeyR {
                            println!("\n[R] Reloading mesh...");
                            self.update_world_mesh();
                        }
                    }
                    ElementState::Released => {
                        self.keys_pressed.remove(&key_code);
                    }
                }
            }
            
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left && state == ElementState::Pressed {
                    self.mouse_captured = !self.mouse_captured;
                    if let Some(window) = &self.window {
                        window.set_cursor_visible(!self.mouse_captured);
                    }
                }
            }
            
            WindowEvent::CursorMoved { position, .. } => {
                if self.mouse_captured {
                    if let Some(last_pos) = self.last_mouse_pos {
                        let dx = (position.x - last_pos.0) as f32;
                        let dy = (position.y - last_pos.1) as f32;
                        
                        self.camera.process_mouse(dx, dy, 0.1);
                    }
                }
                self.last_mouse_pos = Some((position.x, position.y));
            }
            
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size);
                }
                
                // Update camera aspect ratio
                self.camera.aspect = new_size.width as f32 / new_size.height as f32;
            }
            
            WindowEvent::RedrawRequested => {
                let Some(window) = &self.window else { return };
                let Some(renderer) = &mut self.renderer else { return };
                let Some(pipeline) = &self.pipeline else { return };
                
                // Handle input
                let now = std::time::Instant::now();
                let delta_time = now.duration_since(self.last_frame_time).as_secs_f64();
                self.last_frame_time = now;
                
                self.handle_input(delta_time);
                
                // Update camera matrices
                self.camera.update_view_matrix();
                self.camera.update_proj_matrix();
                
                // Render
                let output = renderer.surface.get_current_texture().unwrap();
                let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
                
                let mut encoder = renderer.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Render Encoder"),
                });
                
                {
                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Render Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.53, // Sky blue
                                    g: 0.81,
                                    b: 0.92,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &renderer.depth_texture_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        ..Default::default()
                    });
                    
                    pipeline.render(&mut render_pass, &self.camera, self.vertex_buffer.as_ref(), self.index_buffer.as_ref(), self.num_indices);
                }
                
                renderer.queue.submit(std::iter::once(encoder.finish()));
                output.present();
                
                // FPS counter
                self.frame_count += 1;
                if now.duration_since(self.fps_update_time).as_secs() >= 1 {
                    let fps = self.frame_count as f64 / now.duration_since(self.fps_update_time).as_secs_f64();
                    window.set_title(&format!("Continuous Query Viewer - {:.0} FPS", fps));
                    self.frame_count = 0;
                    self.fps_update_time = now;
                }
                
                window.request_redraw();
            }
            
            _ => {}
        }
    }
}

fn main() {
    println!("=== Continuous Query Viewer - Validation Tool ===");
    println!("Testing continuous query system at Kangaroo Point, Brisbane\n");
    println!("Controls:");
    println!("  WASD - Move forward/back/left/right");
    println!("  Q/E  - Move down/up");
    println!("  Shift - Move faster (500 m/s)");
    println!("  Left Click - Capture/release mouse for camera");
    println!("  R - Reload mesh");
    println!("  ESC - Exit\n");
    
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    
    event_loop.run_app(&mut app).unwrap();
}
