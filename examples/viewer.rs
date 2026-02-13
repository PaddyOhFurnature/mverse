//! Metaverse viewer
//!
//! Opens a window and renders the metaverse world.

use metaverse_core::renderer::{Renderer, pipeline::{BasicPipeline, Vertex}};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::*;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};
use wgpu::util::DeviceExt;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pipeline: Option<BasicPipeline>,
    vertex_buffer: Option<wgpu::Buffer>,
    frame_count: usize,
    fps_update_time: std::time::Instant,
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
            
            // Set identity matrix (no transform)
            pipeline.update_uniforms(&renderer.queue, glam::Mat4::IDENTITY);
            
            self.vertex_buffer = Some(vertex_buffer);
            self.pipeline = Some(pipeline);
            self.renderer = Some(renderer);
            self.window = Some(window);
            self.fps_update_time = std::time::Instant::now();
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
            WindowEvent::RedrawRequested => {
                if let (Some(renderer), Some(window), Some(pipeline), Some(vertex_buffer)) = 
                    (&mut self.renderer, &self.window, &self.pipeline, &self.vertex_buffer) {
                    
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
                    let now = std::time::Instant::now();
                    if now.duration_since(self.fps_update_time).as_secs_f32() >= 1.0 {
                        let fps =
                            self.frame_count as f32 / now.duration_since(self.fps_update_time).as_secs_f32();
                        window.set_title(&format!("Metaverse Viewer - {:.1} FPS", fps));
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
    let mut app = App {
        window: None,
        renderer: None,
        pipeline: None,
        vertex_buffer: None,
        frame_count: 0,
        fps_update_time: std::time::Instant::now(),
    };

    event_loop.run_app(&mut app).unwrap();
}
