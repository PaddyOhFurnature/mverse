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
use metaverse_core::svo::SparseVoxelOctree;
use metaverse_core::terrain::generate_terrain_from_elevation;
use metaverse_core::mesh_generation::{generate_mesh, svo_meshes_to_colored_vertices};
use metaverse_core::materials::MaterialColors;
use metaverse_core::osm_features::carve_river;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
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
    chunk_manager: Option<ChunkManager>,
    full_osm_data: Option<OsmData>, // Keep full dataset for chunk partitioning
    buildings_vertex_buffer: Option<wgpu::Buffer>,
    buildings_index_buffer: Option<wgpu::Buffer>,
    buildings_num_indices: u32,
    show_buildings: bool,
    roads_vertex_buffer: Option<wgpu::Buffer>,
    roads_index_buffer: Option<wgpu::Buffer>,
    roads_num_indices: u32,
    show_roads: bool,
    water_vertex_buffer: Option<wgpu::Buffer>,
    water_index_buffer: Option<wgpu::Buffer>,
    water_num_indices: u32,
    show_water: bool,
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
            roads_vertex_buffer: None,
            roads_index_buffer: None,
            roads_num_indices: 0,
            show_roads: true,
            water_vertex_buffer: None,
            water_index_buffer: None,
            water_num_indices: 0,
            show_water: true,
            downloader: None, // Will be initialized with cache
            chunk_manager: None, // Will be initialized when OSM data loads
            full_osm_data: None, // Will be loaded from cache
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
            
            // Initialize chunk manager
            let chunk_manager = ChunkManager::new(
                9,      // Depth 9 = ~60km chunks (city districts)
                10000.0 // 10km render distance
            );
            
            // Generate mesh using SVO pipeline
            let (mesh_vertices, mesh_indices) = if let Some(ref osm_data) = full_osm_data {
                println!("=== Building SVO World ===");
                
                // Story Bridge area
                let center = GpsPos {
                    lat_deg: -27.463697,
                    lon_deg: 153.035725,
                    elevation_m: 0.0,
                };
                
                // Initialize SRTM
                let cache_for_srtm = DiskCache::new().expect("Failed to create cache");
                let mut srtm = SrtmManager::new(cache_for_srtm);
                srtm.set_network_enabled(false);
                
                // Create SVO for LOD 0 (0-50m range, high detail)
                let depth = 10;  // 1024^3
                let mut svo = SparseVoxelOctree::new(depth);
                let svo_size = 1u32 << depth;
                
                let area_size = 100.0; // 100m coverage (50m radius)
                let voxel_size = area_size / svo_size as f64;
                println!("✓ SVO: {}^3 voxels", svo_size);
                println!("  LOD 0 (0-50m): {:.0}m coverage", area_size);
                println!("  Voxel size: {:.2}cm", voxel_size * 100.0);
                
                // Voxelize terrain
                println!("Voxelizing terrain from SRTM...");
                let elevation_fn = |lat: f64, lon: f64| -> Option<f32> {
                    srtm.get_elevation(lat, lon).map(|e| e as f32)
                };
                
                let coords_fn = |x: u32, y: u32, z: u32| -> GpsPos {
                    let half = svo_size as f64 / 2.0;
                    let dx = (x as f64 - half) * voxel_size;
                    let dy = (y as f64 - half) * voxel_size;
                    let dz = (z as f64 - half) * voxel_size;
                    
                    let lat_deg = center.lat_deg + (dz / 111_000.0);
                    let lon_deg = center.lon_deg + (dx / (111_000.0 * center.lat_deg.to_radians().cos()));
                    let elevation_m = dy;
                    
                    GpsPos { lat_deg, lon_deg, elevation_m }
                };
                
                generate_terrain_from_elevation(&mut svo, elevation_fn, coords_fn, voxel_size);
                println!("✓ Terrain voxelized");
                
                let chunk_center = gps_to_ecef(&center);
                
                // Carve rivers
                if !osm_data.water.is_empty() {
                    println!("Carving {} water features...", osm_data.water.len());
                    for (_i, water) in osm_data.water.iter().enumerate().take(10) {
                        if water.polygon.len() >= 2 {
                            carve_river(&mut svo, &chunk_center, "river", &water.polygon, 30.0, voxel_size);
                        }
                    }
                    println!("✓ Carved {} rivers", 10.min(osm_data.water.len()));
                }
                
                // Add roads (within LOD 0 range)
                use metaverse_core::osm_features::place_road;
                if !osm_data.roads.is_empty() {
                    println!("Placing roads (LOD 0)...");
                    let mut roads_placed = 0;
                    for road in osm_data.roads.iter().take(50) {
                        if road.nodes.len() >= 2 {
                            place_road(&mut svo, &chunk_center, "road", &road.nodes, voxel_size);
                            roads_placed += 1;
                        }
                    }
                    println!("✓ Placed {} roads", roads_placed);
                }
                
                // Add buildings (within LOD 0 range)
                use metaverse_core::osm_features::add_building;
                if !osm_data.buildings.is_empty() {
                    println!("Adding buildings (LOD 0)...");
                    let mut buildings_added = 0;
                    for building in osm_data.buildings.iter().take(100) {
                        if building.polygon.len() >= 3 {
                            add_building(&mut svo, &chunk_center, building, voxel_size);
                            buildings_added += 1;
                        }
                    }
                    println!("✓ Added {} buildings", buildings_added);
                }
                
                // Extract mesh
                println!("Extracting mesh via marching cubes...");
                let meshes = generate_mesh(&svo, 0);
                
                let total_verts: usize = meshes.iter().map(|m| m.vertices.len() / 6).sum();
                println!("✓ Extracted {} material meshes ({} vertices)", meshes.len(), total_verts);
                
                // Convert to GPU format
                let material_colors = MaterialColors::default_palette();
                let (mut vertices, indices) = svo_meshes_to_colored_vertices(&meshes, &material_colors);
                
                // Transform vertices from voxel space to ECEF space
                use metaverse_core::coordinates::{enu_to_ecef, EnuPos};
                let center_ecef = gps_to_ecef(&center);
                let half = svo_size as f32 / 2.0;
                let voxel_to_meters = voxel_size as f32;
                
                for vertex in &mut vertices {
                    // Voxel coords: (0,0,0) to (256,256,256)
                    // Center at (128, 128, 128)
                    // Map to ENU: X=East, Y=Up, Z=North
                    let enu = EnuPos {
                        east: ((vertex.position[0] - half) * voxel_to_meters) as f64,
                        north: ((vertex.position[2] - half) * voxel_to_meters) as f64,
                        up: ((vertex.position[1] - half) * voxel_to_meters) as f64,
                    };
                    
                    let pos_ecef = enu_to_ecef(&enu, &center_ecef, &center);
                    vertex.position = [pos_ecef.x as f32, pos_ecef.y as f32, pos_ecef.z as f32];
                }
                
                println!("✓ {} colored vertices (transformed to ECEF)\n", vertices.len());
                
                (vertices, indices)
            } else {
                println!("No cached OSM data available");
                println!("  Run: cargo run --example download_brisbane_data");
                (Vec::new(), Vec::new())
            };
            
            let buildings_num_indices = mesh_indices.len() as u32;
            
            if !mesh_vertices.is_empty() {
                let buildings_vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("SVO Mesh Vertex Buffer"),
                    contents: bytemuck::cast_slice(&mesh_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                
                let buildings_index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("SVO Mesh Index Buffer"),
                    contents: bytemuck::cast_slice(&mesh_indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                
                self.buildings_vertex_buffer = Some(buildings_vertex_buffer);
                self.buildings_index_buffer = Some(buildings_index_buffer);
                self.buildings_num_indices = buildings_num_indices;
                
                println!("Generated SVO mesh: {} vertices, {} indices", mesh_vertices.len(), mesh_indices.len());
            } else {
                println!("No mesh to render");
            }
            
            // Store full OSM data and chunk manager for dynamic loading
            self.full_osm_data = full_osm_data;
            self.chunk_manager = Some(chunk_manager);
            
            // Clear roads and water buffers (now unified in SVO mesh)
            self.roads_vertex_buffer = None;
            self.roads_index_buffer = None;
            self.roads_num_indices = 0;
            self.water_vertex_buffer = None;
            self.water_index_buffer = None;
            self.water_num_indices = 0;
            
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
                            
                            
                            // Toggle layer visibility
                            if keycode == KeyCode::Digit1 {
                                self.show_buildings = !self.show_buildings;
                                println!("Buildings: {}", if self.show_buildings { "ON" } else { "OFF" });
                            } else if keycode == KeyCode::Digit2 {
                                self.show_roads = !self.show_roads;
                                println!("Roads: {}", if self.show_roads { "ON" } else { "OFF" });
                            } else if keycode == KeyCode::Digit3 {
                                self.show_water = !self.show_water;
                                println!("Water: {}", if self.show_water { "ON" } else { "OFF" });
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
                    
                    let roads_num_indices = self.roads_num_indices;
                    let show_roads = self.show_roads;
                    let roads_vb = self.roads_vertex_buffer.as_ref();
                    let roads_ib = self.roads_index_buffer.as_ref();
                    
                    let water_num_indices = self.water_num_indices;
                    let show_water = self.show_water;
                    let water_vb = self.water_vertex_buffer.as_ref();
                    let water_ib = self.water_index_buffer.as_ref();
                    
                    let result = renderer.render(clear_color, |render_pass| {
                        // Draw water first (bottom layer)
                        if show_water {
                            if let (Some(wvb), Some(wib)) = (water_vb, water_ib) {
                                render_pass.set_pipeline(&pipeline.pipeline);
                                render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                                render_pass.set_vertex_buffer(0, wvb.slice(..));
                                render_pass.set_index_buffer(wib.slice(..), wgpu::IndexFormat::Uint32);
                                render_pass.draw_indexed(0..water_num_indices, 0, 0..1);
                            }
                        }
                        
                        // Draw roads (middle layer)
                        if show_roads {
                            if let (Some(rvb), Some(rib)) = (roads_vb, roads_ib) {
                                render_pass.set_pipeline(&pipeline.pipeline);
                                render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                                render_pass.set_vertex_buffer(0, rvb.slice(..));
                                render_pass.set_index_buffer(rib.slice(..), wgpu::IndexFormat::Uint32);
                                render_pass.draw_indexed(0..roads_num_indices, 0, 0..1);
                            }
                        }
                        
                        // Draw buildings (top layer)
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
