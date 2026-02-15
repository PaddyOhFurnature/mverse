//! Automated screenshot capture matching REFERENCE_IMAGES.md
//! 
//! Takes 10 screenshots from exact positions specified in REFERENCE_IMAGES.md
//! Saves to screenshot/ directory with matching filenames.

use metaverse_core::renderer::{camera::Camera, pipeline::BasicPipeline, Renderer};
use metaverse_core::osm::OsmData;
use metaverse_core::cache::DiskCache;
use metaverse_core::svo_integration::generate_mesh_from_osm;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use std::sync::Arc;
use std::fs;
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes};
use wgpu::util::DeviceExt;
use glam::DVec3;

// Test location: Queen Street Mall, Brisbane
const TEST_GPS: GpsPos = GpsPos {
    lat_deg: -27.469800,
    lon_deg: 153.025100,
    elevation_m: 50.0,
};

// 10 camera views matching REFERENCE_IMAGES.md exactly
struct CameraView {
    filename: &'static str,
    altitude_m: f64,
    heading_deg: f64,   // 0=N, 90=E, 180=S, 270=W
    tilt_deg: f64,      // 0=straight down, 90=horizontal
}

const CAMERA_VIEWS: &[CameraView] = &[
    CameraView { filename: "01_top_down.png", altitude_m: 50.0, heading_deg: 0.0, tilt_deg: 0.0 },
    CameraView { filename: "02_north_horizontal.png", altitude_m: 50.0, heading_deg: 0.0, tilt_deg: 90.0 },
    CameraView { filename: "03_east_horizontal.png", altitude_m: 50.0, heading_deg: 90.0, tilt_deg: 90.0 },
    CameraView { filename: "04_south_horizontal.png", altitude_m: 50.0, heading_deg: 180.0, tilt_deg: 90.0 },
    CameraView { filename: "05_west_horizontal.png", altitude_m: 50.0, heading_deg: 270.0, tilt_deg: 90.0 },
    CameraView { filename: "06_northeast_angle.png", altitude_m: 50.0, heading_deg: 45.0, tilt_deg: 45.0 },
    CameraView { filename: "07_southeast_angle.png", altitude_m: 50.0, heading_deg: 135.0, tilt_deg: 45.0 },
    CameraView { filename: "08_southwest_angle.png", altitude_m: 50.0, heading_deg: 225.0, tilt_deg: 45.0 },
    CameraView { filename: "09_northwest_angle.png", altitude_m: 50.0, heading_deg: 315.0, tilt_deg: 45.0 },
    CameraView { filename: "10_ground_level_north.png", altitude_m: 5.0, heading_deg: 0.0, tilt_deg: 85.0 },
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
        
        // Convert heading and tilt to look direction
        // heading: 0=N, 90=E, 180=S, 270=W
        // tilt: 0=down, 90=horizontal, 180=up
        
        let heading_rad = view.heading_deg.to_radians();
        let tilt_rad = (90.0 - view.tilt_deg).to_radians(); // Convert to pitch (0=horizontal, 90=up, -90=down)
        
        // Direction vector in local NED frame
        let north = tilt_rad.cos() * heading_rad.cos();
        let east = tilt_rad.cos() * heading_rad.sin();
        let down = -tilt_rad.sin();
        
        // Get local ENU frame at camera position
        let lat_rad = camera_gps.lat_deg.to_radians();
        let lon_rad = camera_gps.lon_deg.to_radians();
        
        // ENU basis vectors in ECEF
        let up_ecef = DVec3::new(
            lon_rad.cos() * lat_rad.cos(),
            lon_rad.sin() * lat_rad.cos(),
            lat_rad.sin(),
        );
        let east_ecef = DVec3::new(-lon_rad.sin(), lon_rad.cos(), 0.0);
        let north_ecef = up_ecef.cross(east_ecef);
        
        // Transform local direction to ECEF
        let look_dir = north_ecef * north + east_ecef * east - up_ecef * down;
        let target = position + look_dir.normalize() * 100.0;
        
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
            println!("Location: Queen Street Mall, Brisbane");
            println!("GPS: ({:.6}, {:.6})", TEST_GPS.lat_deg, TEST_GPS.lon_deg);
            println!("Capturing {} views to match REFERENCE_IMAGES.md\n", CAMERA_VIEWS.len());
            
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
            
            // Generate mesh
            let (vertices, indices) = generate_mesh_from_osm(&osm_data);
            println!("Generated {} vertices, {} indices\n", vertices.len(), indices.len());
    
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
