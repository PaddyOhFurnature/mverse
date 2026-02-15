//! Automated screenshot capture matching REFERENCE_IMAGES.md
//! 
//! Takes 10 screenshots from exact positions specified in REFERENCE_IMAGES.md
//! Saves to screenshot/ directory with matching filenames.

use metaverse_core::renderer::{camera::Camera, pipeline::BasicPipeline, Renderer};
use metaverse_core::osm::OsmData;
use metaverse_core::cache::DiskCache;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::svo::SparseVoxelOctree;
use metaverse_core::terrain::generate_terrain_from_elevation;
use metaverse_core::mesh_generation::{generate_mesh, svo_meshes_to_colored_vertices};
use metaverse_core::materials::MaterialColors;
use metaverse_core::osm_features::carve_river;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use std::sync::Arc;
use std::fs;
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes};
use wgpu::util::DeviceExt;
use glam::DVec3;

// Test location: Story Bridge center (actual bridge structure)
// Coordinates from Google Earth: 27°27'49.31"S 153°02'08.61"E
const TEST_GPS: GpsPos = GpsPos {
    lat_deg: -27.463697,  // Story Bridge center (exact)
    lon_deg: 153.035725,
    elevation_m: 250.0,  // 250m altitude for overview
};

// 10 camera views matching REFERENCE_IMAGES.md exactly
struct CameraView {
    filename: &'static str,
    altitude_m: f64,
    heading_deg: f64,   // 0=N, 90=E, 180=S, 270=W
    tilt_deg: f64,      // 0=straight down, 90=horizontal
}

const CAMERA_VIEWS: &[CameraView] = &[
    // 250m altitude - high enough to clear all features, low enough to see detail
    CameraView { filename: "01_top_down.png", altitude_m: 250.0, heading_deg: 0.0, tilt_deg: 0.0 },
    CameraView { filename: "02_north_horizontal.png", altitude_m: 250.0, heading_deg: 0.0, tilt_deg: 90.0 },
    CameraView { filename: "03_east_horizontal.png", altitude_m: 250.0, heading_deg: 90.0, tilt_deg: 90.0 },
    CameraView { filename: "04_south_horizontal.png", altitude_m: 250.0, heading_deg: 180.0, tilt_deg: 90.0 },
    CameraView { filename: "05_west_horizontal.png", altitude_m: 250.0, heading_deg: 270.0, tilt_deg: 90.0 },
    CameraView { filename: "06_northeast_angle.png", altitude_m: 250.0, heading_deg: 45.0, tilt_deg: 45.0 },
    CameraView { filename: "07_southeast_angle.png", altitude_m: 250.0, heading_deg: 135.0, tilt_deg: 45.0 },
    CameraView { filename: "08_southwest_angle.png", altitude_m: 250.0, heading_deg: 225.0, tilt_deg: 45.0 },
    CameraView { filename: "09_northwest_angle.png", altitude_m: 250.0, heading_deg: 315.0, tilt_deg: 45.0 },
    // Ground level at 20m (above street level, can see buildings)
    CameraView { filename: "10_ground_level_north.png", altitude_m: 20.0, heading_deg: 0.0, tilt_deg: 85.0 },
];

struct ScreenshotApp {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pipeline: Option<BasicPipeline>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    
    current_view: usize,
    frames_waited: usize,
    osm_data: Option<OsmData>,
}

impl ScreenshotApp {
    fn new() -> Self {
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            current_view: 0,
            frames_waited: 0,
            osm_data: None,
        }
    }
    
    fn create_camera_for_view(&self, view: &CameraView) -> Camera {
        // Camera position at altitude
        let mut camera_gps = TEST_GPS;
        camera_gps.elevation_m = view.altitude_m;
        let pos_ecef = gps_to_ecef(&camera_gps);
        let position = DVec3::new(pos_ecef.x, pos_ecef.y, pos_ecef.z);
        
        println!("  Camera ECEF: ({:.1}, {:.1}, {:.1})", position.x, position.y, position.z);
        println!("  Camera altitude: {:.1}m", view.altitude_m);
        
        // Calculate local coordinate frame at camera position
        // Up = radial direction away from Earth center
        let up = position.normalize();
        
        // East = perpendicular to up and north pole
        let north_pole = DVec3::new(0.0, 0.0, 1.0);
        let east = north_pole.cross(up).normalize();
        
        // North = perpendicular to up and east (completes right-handed frame)
        let north = up.cross(east);
        
        // Convert heading and tilt to look direction
        // heading: 0=N, 90=E, 180=S, 270=W
        // tilt: 0=straight down, 90=horizontal, 180=straight up
        
        let heading_rad = view.heading_deg.to_radians();
        let tilt_rad = view.tilt_deg.to_radians();
        
        // Horizontal component (in north/east plane)
        let horizontal = north * heading_rad.cos() + east * heading_rad.sin();
        
        // Add vertical component based on tilt
        // tilt=0 -> look down, tilt=90 -> look horizontal, tilt=180 -> look up
        let tilt_from_down = tilt_rad; // 0=down, 90=horizontal, 180=up
        let vertical_component = -up * tilt_from_down.cos(); // Down when tilt=0
        let horizontal_component = horizontal * tilt_from_down.sin(); // 0 when tilt=0
        
        let look_dir = (vertical_component + horizontal_component).normalize();
        let target = position + look_dir * 100.0;
        
        Camera::new(position, target)
    }
    
    fn capture_screenshot(&mut self) {
        if self.current_view >= CAMERA_VIEWS.len() {
            println!("\n✓ All {} screenshots captured!", CAMERA_VIEWS.len());
            std::process::exit(0);
        }
        
        let view = &CAMERA_VIEWS[self.current_view];
        
        if let Some(renderer) = &self.renderer {
            // Create texture to capture to
            let texture_desc = wgpu::TextureDescriptor {
                label: Some("Screenshot Texture"),
                size: wgpu::Extent3d {
                    width: renderer.size.width,
                    height: renderer.size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: renderer.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            };
            
            let texture = renderer.device.create_texture(&texture_desc);
            let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            
            // Create depth texture
            let depth_texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Screenshot Depth Texture"),
                size: wgpu::Extent3d {
                    width: renderer.size.width,
                    height: renderer.size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
            
            // Buffer to copy pixels to
            let bytes_per_row = renderer.size.width * 4; // RGBA8
            let buffer_size = (bytes_per_row * renderer.size.height) as u64;
            
            let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Screenshot Buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            
            // Render to texture
            let camera = self.create_camera_for_view(view);
            let aspect = renderer.size.width as f32 / renderer.size.height as f32;
            let (view_proj, camera_offset) = camera.view_projection_matrix(aspect);
            
            let camera_offset_f32 = glam::Vec3::new(
                camera_offset.x as f32,
                camera_offset.y as f32,
                camera_offset.z as f32,
            );
            let origin_transform = glam::Mat4::from_translation(-camera_offset_f32);
            let final_mvp = view_proj * origin_transform;
            
            if let Some(pipeline) = &self.pipeline {
                pipeline.update_uniforms(&renderer.queue, final_mvp);
                
                let mut encoder = renderer.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Screenshot Encoder"),
                });
                
                {
                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Screenshot Render Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &texture_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.529,
                                    g: 0.808,
                                    b: 0.922,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    
                    if let (Some(vb), Some(ib)) = (&self.vertex_buffer, &self.index_buffer) {
                        render_pass.set_pipeline(&pipeline.pipeline);
                        render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                        render_pass.set_vertex_buffer(0, vb.slice(..));
                        render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                        render_pass.draw_indexed(0..self.num_indices, 0, 0..1);
                    }
                }
                
                // Copy texture to buffer
                encoder.copy_texture_to_buffer(
                    wgpu::ImageCopyTexture {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::ImageCopyBuffer {
                        buffer: &buffer,
                        layout: wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(bytes_per_row),
                            rows_per_image: Some(renderer.size.height),
                        },
                    },
                    texture_desc.size,
                );
                
                renderer.queue.submit(Some(encoder.finish()));
                
                // Map buffer and save image
                let buffer_slice = buffer.slice(..);
                buffer_slice.map_async(wgpu::MapMode::Read, |_| {});
                renderer.device.poll(wgpu::Maintain::Wait);
                
                {
                    let data = buffer_slice.get_mapped_range();
                    
                    // Save as PNG
                    let path = format!("screenshot/{}", view.filename);
                    
                    // Convert BGRA to RGBA if needed
                    let mut rgba_data = vec![0u8; data.len()];
                    for i in (0..data.len()).step_by(4) {
                        rgba_data[i] = data[i + 2];     // R
                        rgba_data[i + 1] = data[i + 1]; // G
                        rgba_data[i + 2] = data[i];     // B
                        rgba_data[i + 3] = data[i + 3]; // A
                    }
                    
                    image::save_buffer(
                        &path,
                        &rgba_data,
                        renderer.size.width,
                        renderer.size.height,
                        image::ColorType::Rgba8,
                    ).expect("Failed to save screenshot");
                    
                    println!("✓ [{}/{}] Saved: {}", 
                        self.current_view + 1, 
                        CAMERA_VIEWS.len(),
                        path);
                }
                
                buffer.unmap();
            }
        }
        
        self.current_view += 1;
        self.frames_waited = 0;
    }
}

impl ApplicationHandler for ScreenshotApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            println!("\n=== AUTOMATED SCREENSHOT CAPTURE ===");
            println!("Location: Story Bridge, Brisbane (elevated bridge with surroundings)");
            println!("GPS: ({:.6}, {:.6})", TEST_GPS.lat_deg, TEST_GPS.lon_deg);
            println!("Features: Bridge, river, tunnels, roads, buildings, parklands");
            println!("Capturing {} views at 250m altitude\n", CAMERA_VIEWS.len());
            
            // Create screenshot directory
            fs::create_dir_all("screenshot").expect("Failed to create screenshot directory");
            
            let window = Arc::new(
                event_loop
                    .create_window(WindowAttributes::default()
                        .with_title("Capturing Screenshots...")
                        .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080)))
                    .unwrap(),
            );

            let renderer = pollster::block_on(Renderer::new(window.clone()));
            let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
            
            // Load OSM data from cache (same as viewer)
            println!("Loading OSM data from cache...");
            
            let cache = DiskCache::new().expect("Failed to create cache");
            let cache_keys = [
                "brisbane_cbd_full_osmdata",
                "brisbane_cbd_osmdata", 
                "brisbane_cbd",
            ];
            
            let mut osm_data: Option<OsmData> = None;
            for cache_key in &cache_keys {
                if let Ok(cached_bytes) = cache.read_osm(cache_key) {
                    if let Ok(data) = serde_json::from_slice::<OsmData>(&cached_bytes) {
                        println!("  Loaded from cache: {}", cache_key);
                        osm_data = Some(data);
                        break;
                    }
                }
            }
            
            let osm_data = osm_data.expect("No OSM data found in cache. Run: cargo run --example viewer");
            println!("  {} buildings", osm_data.buildings.len());
            println!("  {} roads\n", osm_data.roads.len());
            
            // Create SRTM manager for terrain elevation
            let cache_for_srtm = DiskCache::new().expect("Failed to create SRTM cache");
            let mut srtm = SrtmManager::new(cache_for_srtm);
            srtm.set_network_enabled(false); // Use cached tiles only (no network during capture)
            println!("SRTM manager initialized (procedural fallback enabled)");
            
            // === SVO PIPELINE APPROACH ===
            println!("\n=== Building SVO World ===");
            
            // Create SVO (depth 8 = 256^3 for 5km area)
            let depth = 8;
            let mut svo = SparseVoxelOctree::new(depth);
            let svo_size = 1u32 << depth;
            println!("✓ SVO: {}^3 voxels", svo_size);
            
            // Area size and voxel size
            let area_size = 5000.0; // 5km radius to match reference
            let voxel_size = area_size / svo_size as f64;
            println!("  Voxel size: {:.2}m", voxel_size);
            println!("  Coverage: {:.0}m × {:.0}m", area_size, area_size);
            
            // Voxelize terrain from SRTM
            println!("\nVoxelizing terrain from SRTM...");
            let elevation_fn = |lat: f64, lon: f64| -> Option<f32> {
                srtm.get_elevation(lat, lon).map(|e| e as f32)
            };
            
            let center_ground = GpsPos {
                lat_deg: TEST_GPS.lat_deg,
                lon_deg: TEST_GPS.lon_deg,
                elevation_m: 0.0,
            };
            
            let coords_fn = |x: u32, y: u32, z: u32| -> GpsPos {
                let half = svo_size as f64 / 2.0;
                let dx = (x as f64 - half) * voxel_size;
                let dy = (y as f64 - half) * voxel_size;
                let dz = (z as f64 - half) * voxel_size;
                
                let lat_deg = center_ground.lat_deg + (dz / 111_000.0);
                let lon_deg = center_ground.lon_deg + (dx / (111_000.0 * center_ground.lat_deg.to_radians().cos()));
                let elevation_m = dy;
                
                GpsPos { lat_deg, lon_deg, elevation_m }
            };
            
            generate_terrain_from_elevation(&mut svo, elevation_fn, coords_fn, voxel_size);
            println!("✓ Terrain voxelized (STONE/DIRT/AIR)");
            
            let center_ground = GpsPos {
                lat_deg: TEST_GPS.lat_deg,
                lon_deg: TEST_GPS.lon_deg,
                elevation_m: 0.0,
            };
            let chunk_center = gps_to_ecef(&center_ground);
            
            // Carve rivers via CSG
            if !osm_data.water.is_empty() {
                println!("\nCarving {} water features...", osm_data.water.len());
                
                for (i, water) in osm_data.water.iter().enumerate().take(10) {
                    if water.polygon.len() >= 2 {
                        carve_river(&mut svo, &chunk_center, "river", &water.polygon, 30.0, voxel_size);
                    }
                    if (i + 1) % 5 == 0 {
                        println!("  Carved {}/{} rivers", i+1, 10.min(osm_data.water.len()));
                    }
                }
            }
            
            // Add roads
            use metaverse_core::osm_features::place_road;
            if !osm_data.roads.is_empty() {
                println!("\nPlacing {} roads...", osm_data.roads.len());
                let mut roads_placed = 0;
                for road in osm_data.roads.iter().take(1000) {
                    if road.nodes.len() >= 2 {
                        place_road(&mut svo, &chunk_center, "road", &road.nodes, voxel_size);
                        roads_placed += 1;
                        if roads_placed % 200 == 0 {
                            println!("  Placed {}/1000 roads", roads_placed);
                        }
                    }
                }
                println!("✓ Placed {} roads", roads_placed);
            }
            
            // Add buildings
            use metaverse_core::osm_features::add_building;
            if !osm_data.buildings.is_empty() {
                println!("\nAdding {} buildings...", osm_data.buildings.len());
                let mut buildings_added = 0;
                for building in osm_data.buildings.iter().take(5000) {
                    if building.polygon.len() >= 3 {
                        add_building(&mut svo, &chunk_center, building, voxel_size);
                        buildings_added += 1;
                        if buildings_added % 1000 == 0 {
                            println!("  Added {}/5000 buildings", buildings_added);
                        }
                    }
                }
                println!("✓ Added {} buildings", buildings_added);
            }
            
            // Extract mesh via marching cubes
            println!("\nExtracting mesh via marching cubes...");
            let meshes = generate_mesh(&svo, 0);
            
            let total_verts: usize = meshes.iter().map(|m| m.vertices.len() / 6).sum();
            let total_tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
            println!("✓ Extracted {} material meshes:", meshes.len());
            println!("  {} vertices, {} triangles", total_verts, total_tris);
            
            // Convert to ColoredVertex format
            println!("\nConverting to GPU format...");
            let material_colors = MaterialColors::default_palette();
            let (mut vertices, indices) = svo_meshes_to_colored_vertices(&meshes, &material_colors);
            
            // Transform vertices from voxel space to ECEF space
            use metaverse_core::coordinates::{enu_to_ecef, EnuPos};
            let center_ground = GpsPos {
                lat_deg: TEST_GPS.lat_deg,
                lon_deg: TEST_GPS.lon_deg,
                elevation_m: 0.0,  // Mesh at ground level
            };
            let center_ecef = gps_to_ecef(&center_ground);
            let half = svo_size as f32 / 2.0;
            let voxel_to_meters = voxel_size as f32;
            
            println!("  Center ECEF: ({:.1}, {:.1}, {:.1})", center_ecef.x, center_ecef.y, center_ecef.z);
            println!("  Voxel range: 0-{}, centered at {}", svo_size, half);
            println!("  Voxel to meters: {:.2}", voxel_to_meters);
            
            let mut min_pos = [f32::MAX; 3];
            let mut max_pos = [f32::MIN; 3];
            
            for vertex in &mut vertices {
                // Voxel coords: (0,0,0) to (256,256,256)
                // Center at (128, 128, 128)
                // Map to ENU: X=East, Y=Up, Z=North
                let enu = EnuPos {
                    east: ((vertex.position[0] - half) * voxel_to_meters) as f64,
                    north: ((vertex.position[2] - half) * voxel_to_meters) as f64,
                    up: ((vertex.position[1] - half) * voxel_to_meters) as f64,
                };
                
                let pos_ecef = enu_to_ecef(&enu, &center_ecef, &center_ground);
                vertex.position = [pos_ecef.x as f32, pos_ecef.y as f32, pos_ecef.z as f32];
                
                for i in 0..3 {
                    min_pos[i] = min_pos[i].min(vertex.position[i]);
                    max_pos[i] = max_pos[i].max(vertex.position[i]);
                }
            }
            
            println!("  Mesh ECEF bounds:");
            println!("    X: {:.1} to {:.1}", min_pos[0], max_pos[0]);
            println!("    Y: {:.1} to {:.1}", min_pos[1], max_pos[1]);
            println!("    Z: {:.1} to {:.1}", min_pos[2], max_pos[2]);
            
            println!("✓ {} colored vertices (transformed to ECEF), {} indices\n", vertices.len(), indices.len());
    
            // Create buffers
            let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            
            let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            
            self.vertex_buffer = Some(vertex_buffer);
            self.index_buffer = Some(index_buffer);
            self.num_indices = indices.len() as u32;
            self.osm_data = Some(osm_data);
            self.pipeline = Some(pipeline);
            self.renderer = Some(renderer);
            self.window = Some(window);
            
            // Start capturing immediately
            self.capture_screenshot();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            winit::event::WindowEvent::RedrawRequested => {
                // Capture next screenshot
                self.frames_waited += 1;
                if self.frames_waited > 2 {
                    self.capture_screenshot();
                }
                
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = ScreenshotApp::new();
    let _ = event_loop.run_app(&mut app);
}
