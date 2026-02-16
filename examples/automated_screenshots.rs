//! Automated screenshot capture from multiple angles
//! Takes screenshots, saves them, and reports what it sees

use metaverse_core::renderer::{Renderer, pipeline::BasicPipeline};
use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::renderer::camera::Camera;
use metaverse_core::renderer::pipeline::Vertex;
use metaverse_core::coordinates::{gps_to_ecef, ecef_to_gps, GpsPos, EcefPos};
use metaverse_core::svo::AIR;
use std::sync::Arc;
use winit::window::Window;
use wgpu;

// Kangaroo Point, Brisbane
const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

struct ScreenshotCapture {
    renderer: Renderer,
    pipeline: BasicPipeline,
    continuous_world: ContinuousWorld,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
}

impl ScreenshotCapture {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Create headless window
        let event_loop = winit::event_loop::EventLoop::new()?;
        let window = Arc::new(event_loop.create_window(
            winit::window::WindowAttributes::default()
                .with_title("Screenshot Capture")
                .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080))
        )?);
        
        let renderer = Renderer::new(window).await;
        let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
        
        // Create world
        let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
        let center_ecef = gps_to_ecef(&gps_center);
        let center = [center_ecef.x, center_ecef.y, center_ecef.z];
        let continuous_world = ContinuousWorld::new(center, 100.0)?;
        
        Ok(Self {
            renderer,
            pipeline,
            continuous_world,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
        })
    }
    
    fn update_mesh(&mut self, camera_pos: [f64; 3], query_radius: f64) {
        println!("  Querying {}m radius...", query_radius);
        let query = AABB::from_center(camera_pos, query_radius);
        let blocks = self.continuous_world.query_range(query);
        
        println!("  Got {} blocks", blocks.len());
        
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut blocks_with_voxels = 0;
        
        for block in &blocks {
            // Render individual voxels, not the whole block
            for x in 0..8 {
                for y in 0..8 {
                    for z in 0..8 {
                        let voxel_idx = z * 64 + y * 8 + x;
                        let voxel = block.voxels[voxel_idx];
                        
                        if voxel == AIR { continue; }
                        blocks_with_voxels += 1;
                        
                        // Calculate voxel position (1m cubes)
                        let voxel_size = 1.0;
                        let min_x = (block.ecef_min[0] + x as f64) as f32;
                        let min_y = (block.ecef_min[1] + y as f64) as f32;
                        let min_z = (block.ecef_min[2] + z as f64) as f32;
                        let size = voxel_size;
                        
                        let base_idx = vertices.len() as u32;
                        let color = [0.2, 0.2, 0.2, 1.0]; // Dark gray
                        
                        // 8 cube vertices
                        vertices.push(Vertex { position: [min_x, min_y, min_z], normal: [0.0, 0.0, -1.0], color });
                        vertices.push(Vertex { position: [min_x + size, min_y, min_z], normal: [0.0, 0.0, -1.0], color });
                        vertices.push(Vertex { position: [min_x + size, min_y + size, min_z], normal: [0.0, 0.0, -1.0], color });
                        vertices.push(Vertex { position: [min_x, min_y + size, min_z], normal: [0.0, 0.0, -1.0], color });
                        vertices.push(Vertex { position: [min_x, min_y, min_z + size], normal: [0.0, 0.0, 1.0], color });
                        vertices.push(Vertex { position: [min_x + size, min_y, min_z + size], normal: [0.0, 0.0, 1.0], color });
                        vertices.push(Vertex { position: [min_x + size, min_y + size, min_z + size], normal: [0.0, 0.0, 1.0], color });
                        vertices.push(Vertex { position: [min_x, min_y + size, min_z + size], normal: [0.0, 0.0, 1.0], color });
                        
                        // 12 triangles
                        let faces = [
                            [0, 1, 2, 0, 2, 3], // Bottom
                            [4, 6, 5, 4, 7, 6], // Top
                            [0, 4, 5, 0, 5, 1], // Front
                            [1, 5, 6, 1, 6, 2], // Right
                            [2, 6, 7, 2, 7, 3], // Back
                            [3, 7, 4, 3, 4, 0], // Left
                        ];
                        
                        for face in &faces {
                            for &idx in face {
                                indices.push(base_idx + idx);
                            }
                        }
                    }
                }
            }
        }
        
        println!("  {} voxels rendered", blocks_with_voxels);
        println!("  {} vertices, {} triangles", vertices.len(), indices.len() / 3);
        
        if vertices.is_empty() {
            println!("  WARNING: No geometry!");
            return;
        }
        
        // Upload to GPU
        use wgpu::util::DeviceExt;
        
        let vertex_buffer = self.renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        
        let index_buffer = self.renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        
        self.vertex_buffer = Some(vertex_buffer);
        self.index_buffer = Some(index_buffer);
        self.num_indices = indices.len() as u32;
    }
    
    fn capture(&mut self, camera: &Camera, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        println!("\nCapturing: {}", name);
        
        let aspect = self.renderer.size.width as f32 / self.renderer.size.height as f32;
        let (view_proj, camera_offset) = camera.view_projection_matrix(aspect);
        let camera_offset_f32 = glam::Vec3::new(
            camera_offset.x as f32,
            camera_offset.y as f32,
            camera_offset.z as f32,
        );
        let origin_transform = glam::Mat4::from_translation(-camera_offset_f32);
        let final_mvp = view_proj * origin_transform;
        self.pipeline.update_uniforms(&self.renderer.queue, final_mvp);
        
        let clear_color = wgpu::Color { r: 0.7, g: 0.9, b: 0.7, a: 1.0 }; // Light green grass
        let num_indices = self.num_indices;
        let vertex_buffer = self.vertex_buffer.as_ref();
        let index_buffer = self.index_buffer.as_ref();
        
        let result = self.renderer.render_and_capture(clear_color, |render_pass| {
            if let (Some(vb), Some(ib)) = (vertex_buffer, index_buffer) {
                if num_indices > 0 {
                    render_pass.set_pipeline(&self.pipeline.pipeline);
                    render_pass.set_bind_group(0, &self.pipeline.uniform_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, vb.slice(..));
                    render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..num_indices, 0, 0..1);
                }
            }
        });
        
        match result {
            Ok((pixels, width, height)) => {
                let filename = format!("screenshot/{}.png", name);
                image::save_buffer(&filename, &pixels, width, height, image::ColorType::Rgba8)?;
                println!("  ✓ Saved: {}", filename);
                Ok(())
            }
            Err(e) => {
                println!("  ✗ Failed: {}", e);
                Err(e)
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Automated Screenshot Capture ===\n");
    
    let mut capture = ScreenshotCapture::new().await?;
    println!("✓ Initialized\n");
    
    // Start at Kangaroo Point, various altitudes
    let test_positions = vec![
        ("ground_level_from_5m", 5.0, 200.0),  // At ground level, wide radius
        ("low_altitude_20m", 20.0, 100.0),
        ("medium_altitude_50m", 50.0, 150.0),
        ("high_altitude_100m", 100.0, 200.0),
        ("very_high_200m", 200.0, 400.0),
    ];
    
    for (name, altitude, query_radius) in test_positions {
        println!("\n=== {} ===", name);
        
        let gps = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: altitude };
        let pos_ecef = gps_to_ecef(&gps);
        let position = glam::DVec3::new(pos_ecef.x, pos_ecef.y, pos_ecef.z);
        
        // Look down toward Earth center
        let look_at = glam::DVec3::ZERO;
        let camera = Camera::new(position, look_at);
        
        // Update mesh
        capture.update_mesh([pos_ecef.x, pos_ecef.y, pos_ecef.z], query_radius);
        
        // Capture
        capture.capture(&camera, name)?;
    }
    
    // Top-down view from 100m
    println!("\n=== top_down_100m ===");
    let gps = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 100.0 };
    let pos_ecef = gps_to_ecef(&gps);
    let position = glam::DVec3::new(pos_ecef.x, pos_ecef.y, pos_ecef.z);
    
    // Look straight down
    let earth_center = glam::DVec3::ZERO;
    let to_center = (earth_center - position).normalize();
    let look_at = position + to_center * 10.0;
    let camera = Camera::new(position, look_at);
    
    capture.update_mesh([pos_ecef.x, pos_ecef.y, pos_ecef.z], 200.0);
    capture.capture(&camera, "top_down_100m")?;
    
    println!("\n✓ All screenshots captured!");
    println!("\nAnalyze screenshots in: screenshot/");
    
    Ok(())
}
