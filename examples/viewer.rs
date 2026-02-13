//! Metaverse viewer
//!
//! Opens a window and renders the metaverse world.

use metaverse_core::renderer::Renderer;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::*;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
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
            self.renderer = Some(pollster::block_on(Renderer::new(window.clone())));
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
                if let (Some(renderer), Some(window)) = (&mut self.renderer, &self.window) {
                    // Sky blue color
                    let clear_color = wgpu::Color {
                        r: 0.529,
                        g: 0.808,
                        b: 0.922,
                        a: 1.0,
                    };

                    // Render frame
                    match renderer.render(clear_color) {
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
        frame_count: 0,
        fps_update_time: std::time::Instant::now(),
    };

    event_loop.run_app(&mut app).unwrap();
}
