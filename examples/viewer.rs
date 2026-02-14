//! Metaverse viewer
//!
//! Opens a window and renders the metaverse world.

use metaverse_core::renderer::{
    camera::Camera, 
    pipeline::{BasicPipeline, Vertex},
    mesh::generate_buildings_from_osm,
    Renderer
};
use metaverse_core::osm::{load_chunk_osm_data, OverpassClient, OsmData};
use metaverse_core::chunks::ChunkId;
use metaverse_core::cache::DiskCache;
use metaverse_core::elevation_downloader::ElevationDownloader;
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
    downloader: Option<ElevationDownloader>,
    buildings_vertex_buffer: Option<wgpu::Buffer>,
    buildings_index_buffer: Option<wgpu::Buffer>,
    buildings_num_indices: u32,
    show_buildings: bool,
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
        // Start camera at Brisbane at 500m altitude looking straight down
        let camera = Camera::brisbane();
        
        println!("Camera initialized at Brisbane");
        println!("  Position ECEF: ({:.1}, {:.1}, {:.1})",
            camera.position.x, camera.position.y, camera.position.z);
        println!("  Altitude: {:.2} m", camera.position.length() - 6_371_000.0);
        println!("  Looking straight down at ground");
        
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            camera,
            buildings_vertex_buffer: None,
            buildings_index_buffer: None,
            buildings_num_indices: 0,
            show_buildings: true,
            downloader: None, // Will be initialized with cache
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
        
        // Speed modifiers
        let mut speed_mod = 1.0;
        if self.keys_pressed.contains(&KeyCode::ControlLeft) {
            speed_mod *= 0.1; // Slow (0.1x)
        }
        if self.keys_pressed.contains(&KeyCode::ShiftLeft) || self.keys_pressed.contains(&KeyCode::ShiftRight) {
            speed_mod *= 20.0; // Sprint (20x)
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
            self.pipeline = Some(pipeline);
            
            // Initialize elevation downloader with multi-source support
            let cache = DiskCache::new().unwrap();
            let downloader = ElevationDownloader::new(cache);
            
            println!("Elevation downloader initialized");
            println!("  Sources: AWS Terrarium (primary), USGS 3DEP, OpenTopography");
            println!("  Parallel downloads: up to 8 concurrent");
            
            self.downloader = Some(downloader);
            
            // Pre-download Brisbane elevation tiles for building elevation data
            println!("Pre-downloading Brisbane elevation tiles...");
            let brisbane_lat = -27.4698;
            let brisbane_lon = 153.0251;
            let start = std::time::Instant::now();
            
            // Download tiles covering the OSM building area (11x11 grid)
            for lat_offset in -5i32..=5 {
                for lon_offset in -5i32..=5 {
                    let lat = brisbane_lat + lat_offset as f64 * 0.1;
                    let lon = brisbane_lon + lon_offset as f64 * 0.1;
                    self.downloader.as_ref().unwrap().queue_download(lat, lon, 10, 0.0);
                }
            }
            
            // Process all queued downloads synchronously
            while self.downloader.as_ref().unwrap().get_stats().active_downloads > 0 ||
                  self.downloader.as_ref().unwrap().get_stats().queued_downloads > 0 {
                self.downloader.as_ref().unwrap().process_queue();
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            
            let stats = self.downloader.as_ref().unwrap().get_stats();
            println!("  Downloaded {} tiles in {:.1}s ({} successful, {} failed)",
                     stats.downloads_success + stats.downloads_failed,
                     start.elapsed().as_secs_f32(),
                     stats.downloads_success,
                     stats.downloads_failed);
            
            // Load OSM buildings from cache
            println!("Loading OSM buildings from cache...");
            let cache = DiskCache::new().unwrap();
            
            let buildings_color = glam::Vec3::new(0.7, 0.7, 0.8); // Light gray/blue
            
            // Try to load from wide area cache first, then fall back to CBD
            let cache_keys = ["brisbane_wide", "brisbane_cbd"];
            
            let (buildings_vertices, buildings_indices) = {
                let mut result = (Vec::new(), Vec::new());
                for cache_key in &cache_keys {
                    if let Ok(cached_bytes) = cache.read_osm(cache_key) {
                        if let Ok(osm_data) = serde_json::from_slice::<OsmData>(&cached_bytes) {
                            println!("Loaded {} buildings from cache ({})", osm_data.buildings.len(), cache_key);
                            
                            // Generate buildings using OSM elevation data
                            result = generate_buildings_from_osm(&osm_data, buildings_color);
                            break;
                        }
                    }
                }
                if result.0.is_empty() {
                    println!("No cached OSM data available");
                    println!("  Run: cargo run --example download_brisbane_data");
                }
                result
            };
            
            let buildings_num_indices = buildings_indices.len() as u32;
            
            if !buildings_vertices.is_empty() {
                let buildings_vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Buildings Vertex Buffer"),
                    contents: bytemuck::cast_slice(&buildings_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                
                let buildings_index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Buildings Index Buffer"),
                    contents: bytemuck::cast_slice(&buildings_indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                
                self.buildings_vertex_buffer = Some(buildings_vertex_buffer);
                self.buildings_index_buffer = Some(buildings_index_buffer);
                self.buildings_num_indices = buildings_num_indices;
                
                println!("Generated {} building vertices, {} indices", buildings_vertices.len(), buildings_indices.len());
            } else {
                println!("No buildings to render");
            }
            
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
                            
                            
                            // Toggle buildings visibility
                            if keycode == KeyCode::Digit1 {
                                self.show_buildings = !self.show_buildings;
                                println!("Buildings: {}", if self.show_buildings { "ON" } else { "OFF" });
                            }
                            
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
                
                // Process elevation download queue
                if let Some(downloader) = &self.downloader {
                    downloader.process_queue();
                }
                
                if let (Some(renderer), Some(window), Some(pipeline)) = 
                    (&mut self.renderer, &self.window, &self.pipeline) {
                    
                    // Update camera matrix with floating origin
                    let aspect = renderer.size.width as f32 / renderer.size.height as f32;
                    let (view_proj, camera_offset) = self.camera.view_projection_matrix(aspect);
                    
                    // The meshes are in absolute ECEF coordinates (millions of meters)
                    // We need to translate everything by -camera_offset to center the world around camera
                    // Transform order: model coords → subtract camera offset → view → projection
                    let camera_offset_f32 = glam::Vec3::new(
                        camera_offset.x as f32,
                        camera_offset.y as f32,
                        camera_offset.z as f32,
                    );
                    // Create a translation matrix that shifts world by -camera position
                    let origin_transform = glam::Mat4::from_translation(-camera_offset_f32);
                    // Apply: projection * view * translate
                    let final_mvp = view_proj * origin_transform;
                    
                    pipeline.update_uniforms(&renderer.queue, final_mvp);
                    
                    // Sky blue color
                    let clear_color = wgpu::Color {
                        r: 0.529,
                        g: 0.808,
                        b: 0.922,
                        a: 1.0,
                    };

                    // Render frame
                    let buildings_num_indices = self.buildings_num_indices;
                    let show_buildings = self.show_buildings;
                    let buildings_vb = self.buildings_vertex_buffer.as_ref();
                    let buildings_ib = self.buildings_index_buffer.as_ref();
                    
                    let result = renderer.render(clear_color, |render_pass| {
                        // Draw buildings
                        if show_buildings {
                            if let (Some(bvb), Some(bib)) = (buildings_vb, buildings_ib) {
                                render_pass.set_pipeline(&pipeline.pipeline);
                                render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                                render_pass.set_vertex_buffer(0, bvb.slice(..));
                                render_pass.set_index_buffer(bib.slice(..), wgpu::IndexFormat::Uint32);
                                render_pass.draw_indexed(0..buildings_num_indices, 0, 0..1);
                            }
                        }
                    });

                    match result {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => renderer.resize(renderer.size),
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => eprintln!("Render error: {:?}", e),
                    }

                    // Update FPS counter and stats
                    self.frame_count += 1;
                    if now.duration_since(self.fps_update_time).as_secs_f32() >= 1.0 {
                        let fps =
                            self.frame_count as f32 / now.duration_since(self.fps_update_time).as_secs_f32();
                        
                        // Get download stats
                        let stats = if let Some(downloader) = &self.downloader {
                            let s = downloader.get_stats();
                            format!(" | DL: {}↓ {}✓ {}✗ Q:{}", 
                                s.active_downloads,
                                s.downloads_success,
                                s.downloads_failed,
                                s.queued_downloads)
                        } else {
                            String::new()
                        };
                        
                        // Show camera position
                        let pos = self.camera.position;
                        let alt = pos.length() - 6_371_000.0;
                        window.set_title(&format!(
                            "Metaverse Viewer - {:.1} FPS | Alt: {:.0}m | Speed: {:.1}x{}",
                            fps, alt, self.camera.speed_multiplier, stats
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
    println!("  Q/E or Space - Move down/up");
    println!("  Shift (either) - 20x sprint speed");
    println!("  Ctrl - 0.1x slow speed");
    println!("  Mouse - Look around (click to capture)");
    println!("  [ ] - Decrease/increase tile depth (0-5)");
    println!("  1 - Toggle sphere visibility");
    println!("  2 - Toggle tile outlines");
    println!("  3 - Toggle terrain patches");
    println!("  4 - Toggle OSM buildings");
    println!("  Escape - Release mouse");
    println!();

    event_loop.run_app(&mut app).unwrap();
}
