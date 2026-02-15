//! Metaverse viewer
//!
//! Opens a window and renders the metaverse world.

use metaverse_core::renderer::{
    camera::Camera, 
    pipeline::BasicPipeline,
    Renderer
};
use metaverse_core::osm::OsmData;
use metaverse_core::cache::DiskCache;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::elevation_downloader::ElevationDownloader;
use metaverse_core::chunk_manager::ChunkManager;
use metaverse_core::world_manager::WorldManager;
use metaverse_core::mesh_generation::svo_meshes_to_colored_vertices;
use metaverse_core::materials::MaterialColors;
use metaverse_core::coordinates::{gps_to_ecef, enu_to_ecef, ecef_to_gps, GpsPos, EnuPos, EcefPos};
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
    world_manager: Option<WorldManager>,
    srtm: Option<SrtmManager>,
    full_osm_data: Option<OsmData>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    frame_count: usize,
    fps_update_time: std::time::Instant,
    last_frame_time: std::time::Instant,
    chunk_update_frame: usize, // Frame number when chunks were last updated
    
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
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            downloader: None,
            world_manager: None,
            srtm: None,
            full_osm_data: None,
            frame_count: 0,
            fps_update_time: std::time::Instant::now(),
            last_frame_time: std::time::Instant::now(),
            chunk_update_frame: 0,
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
    
    fn update_world_chunks(&mut self) {
        println!("[update_world_chunks] Starting...");
        
        // Update WorldManager and extract visible meshes
        if let (Some(world_manager), Some(srtm), Some(osm_data), Some(renderer)) = 
            (&mut self.world_manager, &mut self.srtm, &self.full_osm_data, &self.renderer) {
            
            println!("[update_world_chunks] All components present");
            
            // Convert camera to EcefPos
            let camera_ecef = EcefPos {
                x: self.camera.position.x,
                y: self.camera.position.y,
                z: self.camera.position.z,
            };
            
            println!("[update_world_chunks] Camera ECEF: ({:.1}, {:.1}, {:.1})", 
                camera_ecef.x, camera_ecef.y, camera_ecef.z);
            
            // Update chunks based on camera position
            let num_chunks = world_manager.update(&camera_ecef, srtm, osm_data);
            println!("[update_world_chunks] After update: {} chunks loaded", num_chunks);
            
            // Extract all visible chunks with their meshes
            let chunk_meshes = world_manager.extract_meshes(&camera_ecef);
            println!("[update_world_chunks] Extracted {} chunk meshes", chunk_meshes.len());
            
            if chunk_meshes.is_empty() {
                println!("[update_world_chunks] No chunk meshes - nothing to render");
                return;
            }
            
            println!("[update_world_chunks] Processing {} chunks", chunk_meshes.len());
            
            // Convert all chunk meshes to GPU format and transform to ECEF
            let material_colors = MaterialColors::default_palette();
            let mut all_vertices = Vec::new();
            let mut all_indices = Vec::new();
            
            for (meshes, chunk_center) in chunk_meshes {
                if meshes.is_empty() {
                    continue;
                }
                
                // Convert meshes to colored vertices (still in voxel space)
                let (mut vertices, indices) = svo_meshes_to_colored_vertices(&meshes, &material_colors);
                
                // Transform from voxel space to ECEF
                // Voxels are centered at chunk_center with voxel_size spacing
                let svo_size = 1u32 << world_manager.svo_depth();
                let voxel_size = 1000.0 / svo_size as f64; // ~1km chunk / 1024 voxels = ~1m voxels
                let half = svo_size as f32 / 2.0;
                let voxel_to_meters = voxel_size as f32;
                
                // Convert chunk center to GPS for ENU transform
                let center_gps = ecef_to_gps(&chunk_center);
                
                for vertex in &mut vertices {
                    // Map voxel coords to ENU relative to chunk center
                    let enu = EnuPos {
                        east: ((vertex.position[0] - half) * voxel_to_meters) as f64,
                        north: ((vertex.position[2] - half) * voxel_to_meters) as f64,
                        up: ((vertex.position[1] - half) * voxel_to_meters) as f64,
                    };
                    
                    // Transform to ECEF
                    let pos_ecef = enu_to_ecef(&enu, &chunk_center, &center_gps);
                    vertex.position = [pos_ecef.x as f32, pos_ecef.y as f32, pos_ecef.z as f32];
                }
                
                // Append to combined buffers (adjust indices for offset)
                let vertex_offset = all_vertices.len() as u32;
                all_vertices.extend(vertices);
                all_indices.extend(indices.iter().map(|i| i + vertex_offset));
            }
            
            if all_vertices.is_empty() || all_indices.is_empty() {
                println!("[update_world_chunks] No vertices/indices generated");
                return;
            }
            
            println!("[update_world_chunks] Generated {} vertices, {} indices", 
                all_vertices.len(), all_indices.len());
            
            // Update GPU buffers
            let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Mesh Vertex Buffer"),
                contents: bytemuck::cast_slice(&all_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            
            let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Mesh Index Buffer"),
                contents: bytemuck::cast_slice(&all_indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            
            self.vertex_buffer = Some(vertex_buffer);
            self.index_buffer = Some(index_buffer);
            self.num_indices = all_indices.len() as u32;
        }
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
            println!("Creating renderer...");
            let renderer = pollster::block_on(Renderer::new(window.clone()));
            println!("✓ Renderer created");
            
            // Create pipeline
            println!("Creating pipeline...");
            let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
            println!("✓ Pipeline created");
            
            // Initialize elevation downloader with multi-source support
            println!("Creating cache...");
            let cache = DiskCache::new().unwrap();
            println!("✓ Cache created");
            
            println!("Creating elevation downloader...");
            let downloader = ElevationDownloader::new(cache);
            println!("✓ Downloader created");
            
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
            
            // Load OSM data and initialize chunk manager
            println!("Loading OSM data from cache...");
            let cache = DiskCache::new().unwrap();
            println!("[DEBUG] Cache created");
            
            // Try to load from wide area cache first, then fall back to CBD
            let cache_keys = ["brisbane_wide", "brisbane_cbd"];
            
            let mut full_osm_data: Option<OsmData> = None;
            for cache_key in &cache_keys {
                println!("[DEBUG] Trying cache key: {}", cache_key);
                if let Ok(cached_bytes) = cache.read_osm(cache_key) {
                    println!("[DEBUG] Found {} bytes", cached_bytes.len());
                    if let Ok(osm_data) = serde_json::from_slice::<OsmData>(&cached_bytes) {
                        println!("Loaded {} buildings, {} roads, {} water, {} parks from cache ({})", 
                               osm_data.buildings.len(), osm_data.roads.len(), 
                               osm_data.water.len(), osm_data.parks.len(), cache_key);
                        full_osm_data = Some(osm_data);
                        break;
                    } else {
                        println!("[DEBUG] Failed to parse JSON");
                    }
                } else {
                    println!("[DEBUG] Cache key not found");
                }
            }
            
            // Initialize SRTM for terrain data
            let cache_for_srtm = DiskCache::new().expect("Failed to create cache for SRTM");
            let mut srtm = SrtmManager::new(cache_for_srtm);
            srtm.set_network_enabled(false);
            
            // Initialize WorldManager for chunk streaming
            let world_manager = if full_osm_data.is_some() {
                println!("=== Initializing WorldManager ===");
                let wm = WorldManager::new(
                    14,     // Depth 14 chunks (~400m per tile as per GLOSSARY.md)
                    2000.0, // 2km render distance
                    9       // SVO depth 9 (512³ voxels = ~0.78m voxels for 400m chunk, supports LOD 0-3)
                );
                println!("✓ WorldManager initialized");
                Some(wm)
            } else {
                println!("No cached OSM data - WorldManager not initialized");
                println!("  Run: cargo run --example download_brisbane_data");
                None
            };
            
            println!("Creating GPU buffers...");
            // Initial mesh will be empty - updated each frame
            let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Empty Vertex Buffer"),
                contents: &[],
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            
            let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Empty Index Buffer"),
                contents: &[],
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            });
            println!("✓ GPU buffers created");
            
            // Store for runtime updates
            println!("Storing state...");
            self.world_manager = world_manager;
            self.srtm = Some(srtm);
            self.full_osm_data = full_osm_data;
            self.vertex_buffer = Some(vertex_buffer);
            self.index_buffer = Some(index_buffer);
            self.num_indices = 0;
            
            self.pipeline = Some(pipeline);
            self.renderer = Some(renderer);
            self.window = Some(window);
            self.fps_update_time = std::time::Instant::now();
            self.last_frame_time = std::time::Instant::now();
            println!("✓ Initialization complete - ready to render!");
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
                            
                            // Mouse capture toggle
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
                
                // Update world chunks based on camera position
                // Update immediately on first frame, then every 30 frames after that
                if self.frame_count == 0 || (self.frame_count - self.chunk_update_frame) >= 30 {
                    println!("[Frame {}] Updating world chunks...", self.frame_count);
                    self.update_world_chunks();
                    self.chunk_update_frame = self.frame_count;
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

                    // Render unified world mesh
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
                        
                        // Show camera position with GPS coordinates
                        use metaverse_core::coordinates::{ecef_to_gps, EcefPos};
                        let pos = self.camera.position;
                        let ecef = EcefPos { x: pos.x, y: pos.y, z: pos.z };
                        let gps = ecef_to_gps(&ecef);
                        let alt = gps.elevation_m;
                        
                        window.set_title(&format!(
                            "Metaverse Viewer - {:.1} FPS | GPS: ({:.6}, {:.6}) | Alt: {:.1}m | Speed: {:.1}x{}",
                            fps, gps.lat_deg, gps.lon_deg, alt, self.camera.speed_multiplier, stats
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
    println!("  1 - Toggle buildings");
    println!("  2 - Toggle roads");
    println!("  3 - Toggle water");
    println!("  Escape - Release mouse");
    println!();

    event_loop.run_app(&mut app).unwrap();
}
