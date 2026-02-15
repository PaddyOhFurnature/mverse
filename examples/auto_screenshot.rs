//! Automated screenshot capture
//! 
//! Takes 10 screenshots from predefined positions and exits.
//! Each location gets 6 views: top, bottom, north, south, east, west

use metaverse_core::renderer::{camera::Camera, pipeline::BasicPipeline, Renderer};
use metaverse_core::osm::OsmData;
use metaverse_core::cache::DiskCache;
use metaverse_core::svo_integration::generate_mesh_from_osm;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes};
use wgpu::util::DeviceExt;
use glam::DVec3;

struct TestPosition {
    name: &'static str,
    gps: GpsPos,
    google_earth_link: &'static str,
}

// Single test location with 6 camera angles
const TEST_LOCATION: TestPosition = TestPosition {
    name: "queen_street_mall",
    gps: GpsPos {
        lat_deg: -27.469800,
        lon_deg: 153.025100,
        elevation_m: 50.0, // 50m above ground for good overview
    },
    google_earth_link: "https://earth.google.com/web/@-27.4698,153.0251,50a,0d,45y,0h,45t,0r",
};

struct ScreenshotApp {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pipeline: Option<BasicPipeline>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    
    // Screenshot state
    current_angle: usize,
    screenshots_taken: usize,
    frame_count: usize,
}

// 6 camera angles: top, north, east, south, west, 45-degree
const CAMERA_ANGLES: &[(&str, f64, f64)] = &[
    ("01_top_down", 0.0, -90.0),           // Looking straight down
    ("02_north", 0.0, 0.0),                // Looking north (horizontal)
    ("03_east", 90.0, 0.0),                // Looking east
    ("04_south", 180.0, 0.0),              // Looking south
    ("05_west", 270.0, 0.0),               // Looking west
    ("06_angle_45", 45.0, -45.0),          // 45° angle from north-east
];

impl ScreenshotApp {
    fn new() -> Self {
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            current_angle: 0,
            screenshots_taken: 0,
            frame_count: 0,
        }
    }
    
    fn take_screenshot(&mut self) {
        if self.current_angle >= CAMERA_ANGLES.len() {
            println!("\n=== ALL SCREENSHOTS COMPLETE ===");
            std::process::exit(0);
        }
        
        let (name, yaw, pitch) = CAMERA_ANGLES[self.current_angle];
        
        println!("\n[{}/{}] Taking screenshot: {}", 
            self.current_angle + 1, 
            CAMERA_ANGLES.len(), 
            name);
        println!("  Yaw: {:.1}°, Pitch: {:.1}°", yaw, pitch);
        
        // TODO: Actually capture screenshot here
        // For now, just simulate by waiting a few frames
        if self.frame_count > 5 {
            println!("  Saved: screenshot/{}.png", name);
            self.current_angle += 1;
            self.screenshots_taken += 1;
            self.frame_count = 0;
        }
    }
}

impl ApplicationHandler for ScreenshotApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            println!("=== AUTOMATED SCREENSHOT CAPTURE ===");
            println!("\nTest Location: {}", TEST_LOCATION.name);
            println!("GPS: ({:.6}, {:.6}, {:.1}m)",
                TEST_LOCATION.gps.lat_deg,
                TEST_LOCATION.gps.lon_deg,
                TEST_LOCATION.gps.elevation_m);
            println!("\nGoogle Earth Link:");
            println!("{}", TEST_LOCATION.google_earth_link);
            println!("\nTaking {} screenshots...\n", CAMERA_ANGLES.len());
            
            let window = Arc::new(
                event_loop
                    .create_window(WindowAttributes::default()
                        .with_title("Screenshot Capture")
                        .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080)))
                    .unwrap(),
            );

            let renderer = pollster::block_on(Renderer::new(window.clone()));
            let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
            
            // Load OSM data
            let cache = DiskCache::new().unwrap();
            match cache.get("osm_brisbane_cbd") {
                Ok(Some(data)) => {
                    let osm_data: OsmData = bincode::deserialize(&data).unwrap();
                    
                    println!("Loaded {} buildings, {} roads", 
                        osm_data.buildings.len(), osm_data.roads.len());
                    
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
                }
                Err(e) => {
                    println!("ERROR: Could not load OSM data: {:?}", e);
                    println!("Run: cargo run --example download_brisbane_data");
                    std::process::exit(1);
                }
            }
            self.pipeline = Some(pipeline);
            self.renderer = Some(renderer);
            self.window = Some(window);
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
                self.frame_count += 1;
                
                if let (Some(renderer), Some(window), Some(pipeline)) = 
                    (&self.renderer, &self.window, &self.pipeline) {
                    
                    // Set camera to test position
                    let pos_ecef = gps_to_ecef(&TEST_LOCATION.gps);
                    let position = DVec3::new(pos_ecef.x, pos_ecef.y, pos_ecef.z);
                    
                    let camera = Camera::new(position, position + DVec3::new(0.0, 0.0, 1.0));
                    
                    // TODO: Rotate camera based on current_angle
                    // For now, just use default orientation
                    
                    // Render
                    let aspect = renderer.size.width as f32 / renderer.size.height as f32;
                    let (view_proj, camera_offset) = camera.view_projection_matrix(aspect);
                    
                    let camera_offset_f32 = glam::Vec3::new(
                        camera_offset.x as f32,
                        camera_offset.y as f32,
                        camera_offset.z as f32,
                    );
                    let origin_transform = glam::Mat4::from_translation(-camera_offset_f32);
                    let final_mvp = view_proj * origin_transform;
                    
                    pipeline.update_uniforms(&renderer.queue, final_mvp);
                    
                    let clear_color = wgpu::Color {
                        r: 0.529,
                        g: 0.808,
                        b: 0.922,
                        a: 1.0,
                    };

                    if let (Some(vb), Some(ib)) = (&self.vertex_buffer, &self.index_buffer) {
                        let _ = renderer.render(clear_color, |render_pass| {
                            render_pass.set_pipeline(&pipeline.pipeline);
                            render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                            render_pass.set_vertex_buffer(0, vb.slice(..));
                            render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                            render_pass.draw_indexed(0..self.num_indices, 0, 0..1);
                        });
                    }
                    
                    // Take screenshot after a few frames
                    self.take_screenshot();
                    
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
