//! Metaverse viewer
//!
//! Opens a window and renders the metaverse world.

use metaverse_core::renderer::{
    camera::Camera, 
    pipeline::{BasicPipeline, Vertex},
    Renderer
};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::*;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};
use wgpu::util::DeviceExt;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pipeline: Option<BasicPipeline>,
    camera: Camera,
    vertex_buffer: Option<wgpu::Buffer>,
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
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            camera: Camera::brisbane(),
            vertex_buffer: None,
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
        if self.keys_pressed.contains(&KeyCode::KeyQ) || self.keys_pressed.contains(&KeyCode::ShiftLeft) {
            up -= 1.0;
        }
        
        // Speed modifiers
        let mut speed_mod = 1.0;
        if self.keys_pressed.contains(&KeyCode::ControlLeft) {
            speed_mod *= 0.1; // Slow
        }
        if self.keys_pressed.contains(&KeyCode::ShiftRight) {
            speed_mod *= 10.0; // Fast
        }
        
        let original_multiplier = self.camera.speed_multiplier;
        self.camera.speed_multiplier *= speed_mod;
        
        if forward != 0.0 || right != 0.0 || up != 0.0 {
            self.camera.move_relative(forward, right, up, delta_time);
        }
        
        self.camera.speed_multiplier = original_multiplier;
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = Arc::new(
                event_loop
                    .create_window(
                        WindowAttributes::default()
                            .with_title("Metaverse Viewer")
                            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                    )
                    .unwrap(),
            );

            // Create renderer
            let renderer = pollster::block_on(Renderer::new(window.clone()));
            
            // Create pipeline
            let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
            
            // Create a test triangle
            let vertices = vec![
                Vertex {
                    position: [0.0, 0.5, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [1.0, 0.0, 0.0, 1.0], // Red
                },
                Vertex {
                    position: [-0.5, -0.5, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [0.0, 1.0, 0.0, 1.0], // Green
                },
                Vertex {
                    position: [0.5, -0.5, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [0.0, 0.0, 1.0, 1.0], // Blue
                },
            ];
            
            let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Triangle Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            
            self.vertex_buffer = Some(vertex_buffer);
            self.pipeline = Some(pipeline);
            self.renderer = Some(renderer);
            self.window = Some(window);
            self.fps_update_time = std::time::Instant::now();
            self.last_frame_time = std::time::Instant::now();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(physical_size);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(keycode) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            self.keys_pressed.insert(keycode);
                            
                            // Toggle mouse capture on click
                            if keycode == KeyCode::Escape {
                                self.mouse_captured = false;
                                if let Some(window) = &self.window {
                                    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                    window.set_cursor_visible(true);
                                }
                            }
                        }
                        ElementState::Released => {
                            self.keys_pressed.remove(&keycode);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                // Capture mouse on left click
                if !self.mouse_captured {
                    self.mouse_captured = true;
                    if let Some(window) = &self.window {
                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                        window.set_cursor_visible(false);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.mouse_captured {
                    if let Some(last_pos) = self.last_mouse_pos {
                        let delta_x = position.x - last_pos.0;
                        let delta_y = position.y - last_pos.1;
                        
                        // Mouse sensitivity
                        let sensitivity = 0.002;
                        let yaw_delta = -delta_x * sensitivity;
                        let pitch_delta = -delta_y * sensitivity;
                        
                        self.camera.rotate(pitch_delta, yaw_delta);
                    }
                    self.last_mouse_pos = Some((position.x, position.y));
                } else {
                    self.last_mouse_pos = None;
                }
            }
            WindowEvent::RedrawRequested => {
                // Calculate delta time
                let now = std::time::Instant::now();
                let delta_time = now.duration_since(self.last_frame_time).as_secs_f64();
                self.last_frame_time = now;
                
                // Handle input
                self.handle_input(delta_time);
                
                if let (Some(renderer), Some(window), Some(pipeline), Some(vertex_buffer)) = 
                    (&mut self.renderer, &self.window, &self.pipeline, &self.vertex_buffer) {
                    
                    // Update camera matrix
                    let aspect = renderer.size.width as f32 / renderer.size.height as f32;
                    let (view_proj, _offset) = self.camera.view_projection_matrix(aspect);
                    pipeline.update_uniforms(&renderer.queue, view_proj);
                    
                    // Sky blue color
                    let clear_color = wgpu::Color {
                        r: 0.529,
                        g: 0.808,
                        b: 0.922,
                        a: 1.0,
                    };

                    // Render frame with triangle
                    let result = renderer.render(clear_color, |render_pass| {
                        render_pass.set_pipeline(&pipeline.pipeline);
                        render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                        render_pass.draw(0..3, 0..1);
                    });

                    match result {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => renderer.resize(renderer.size),
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => eprintln!("Render error: {:?}", e),
                    }

                    // Update FPS counter
                    self.frame_count += 1;
                    if now.duration_since(self.fps_update_time).as_secs_f32() >= 1.0 {
                        let fps =
                            self.frame_count as f32 / now.duration_since(self.fps_update_time).as_secs_f32();
                        
                        // Also show camera position
                        let pos = self.camera.position;
                        let alt = pos.length() - 6_371_000.0;
                        window.set_title(&format!(
                            "Metaverse Viewer - {:.1} FPS | Alt: {:.0}m | Speed: {:.1}x",
                            fps, alt, self.camera.speed_multiplier
                        ));
                        
                        self.frame_count = 0;
                        self.fps_update_time = now;
                    }

                    // Request next frame
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    env_logger::init();

    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();

    println!("=== Metaverse Viewer ===");
    println!("Controls:");
    println!("  WASD - Move forward/left/back/right");
    println!("  Q/E or Shift/Space - Move down/up");
    println!("  Right Shift - 10x speed boost");
    println!("  Left Ctrl - 0.1x speed (slow)");
    println!("  Left Click - Capture mouse for look");
    println!("  Escape - Release mouse");
    println!();

    event_loop.run_app(&mut app).unwrap();
}
