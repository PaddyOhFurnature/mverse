//! Headless terrain screenshot capture
//!
//! Generates terrain and saves screenshot automatically for validation.

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource, OpenTopographySource},
    marching_cubes::extract_cube_mesh,
    materials::MaterialId,
    mesh::Mesh,
    renderer::{Camera, MeshBuffer, RenderContext, RenderPipeline},
    terrain::TerrainGenerator,
    voxel::{Octree, VoxelCoord},
};
use glam::Vec3;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    env_logger::init();
    
    println!("\n=== Terrain Screenshot Tool ===");
    println!("Generating terrain and capturing screenshot...\n");
    
    // Create offscreen window
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let window = event_loop
        .create_window(
            winit::window::WindowAttributes::default()
                .with_title("Screenshot Capture")
                .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080))
                .with_visible(false)  // Hidden for headless
        )
        .unwrap();
    
    let window = Arc::new(window);
    
    // Initialize wgpu
    println!("Initializing wgpu...");
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let pipeline = RenderPipeline::new(&context);
    
    // Setup camera
    let aspect = 1920.0 / 1080.0;
    let mut camera = Camera::new(
        Vec3::new(5.0, 30.0, -20.0),  // View from angle
        aspect,
    );
    camera.yaw = 0.5;  // Look slightly to the side
    camera.pitch = -0.3;  // Look down at terrain
    
    // Generate terrain
    println!("Generating terrain...");
    let start = Instant::now();
    let terrain_mesh = generate_terrain_mesh();
    println!("Generated in {:.2}s", start.elapsed().as_secs_f64());
    println!("  Vertices: {}", terrain_mesh.vertex_count());
    println!("  Triangles: {}", terrain_mesh.triangle_count());
    
    if terrain_mesh.is_empty() {
        eprintln!("ERROR: No mesh generated!");
        return;
    }
    
    // Upload to GPU
    println!("\nUploading to GPU...");
    let mesh_buffer = MeshBuffer::from_mesh(&context.device, &terrain_mesh);
    
    // Render frame
    println!("Rendering frame...");
    pipeline.update_camera(&context.queue, &camera);
    
    let output = context.surface.get_current_texture().unwrap();
    let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
    
    let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Render Encoder"),
    });
    
    {
        let mut render_pass = pipeline.begin_frame(&mut encoder, &view);
        pipeline.set_pipeline(&mut render_pass);
        mesh_buffer.render(&mut render_pass);
    }
    
    // Create texture for screenshot
    // Use surface format for compatibility with pipeline
    let texture_desc = wgpu::TextureDescriptor {
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: context.surface.get_capabilities(&context.adapter).formats[0],
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
        label: None,
        view_formats: &[],
    };
    let texture = context.device.create_texture(&texture_desc);
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    
    // Create depth buffer for screenshot
    let depth_texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Screenshot Depth Texture"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
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
    
    // Render to texture
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Screenshot Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.1,
                        g: 0.2,
                        b: 0.3,
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
        
        render_pass.set_pipeline(pipeline.pipeline());
        render_pass.set_bind_group(0, pipeline.camera_bind_group(), &[]);
        mesh_buffer.render(&mut render_pass);
    }
    
    // Copy to buffer
    let buffer_dimensions = BufferDimensions::new(1920, 1080);
    let output_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        size: (buffer_dimensions.padded_bytes_per_row * buffer_dimensions.height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        label: None,
        mapped_at_creation: false,
    });
    
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            aspect: wgpu::TextureAspect::All,
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
        },
        wgpu::ImageCopyBuffer {
            buffer: &output_buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(buffer_dimensions.padded_bytes_per_row),
                rows_per_image: Some(buffer_dimensions.height),
            },
        },
        texture_desc.size,
    );
    
    context.queue.submit(Some(encoder.finish()));
    
    // Wait for GPU
    let buffer_slice = output_buffer.slice(..);
    buffer_slice.map_async(wgpu::MapMode::Read, |_| {});
    context.device.poll(wgpu::Maintain::Wait);
    
    // Save to PNG
    println!("\nSaving screenshot...");
    let data = buffer_slice.get_mapped_range();
    
    let mut png_data = Vec::with_capacity((1920 * 1080 * 4) as usize);
    for chunk in data.chunks(buffer_dimensions.padded_bytes_per_row as usize) {
        png_data.extend_from_slice(&chunk[..buffer_dimensions.unpadded_bytes_per_row as usize]);
    }
    
    let path = PathBuf::from("screenshot/terrain_validation.png");
    std::fs::create_dir_all("screenshot").ok();
    
    image::save_buffer(
        &path,
        &png_data,
        1920,
        1080,
        image::ColorType::Rgba8,
    )
    .unwrap();
    
    println!("✓ Screenshot saved to: {}", path.display());
    println!("\nDone!");
}

struct BufferDimensions {
    width: u32,
    height: u32,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
}

impl BufferDimensions {
    fn new(width: u32, height: u32) -> Self {
        let bytes_per_pixel = 4; // RGBA8
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) / align * align;
        
        Self {
            width,
            height,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
        }
    }
}

fn generate_terrain_mesh() -> Mesh {
    // Setup elevation pipeline
    let nas = NasFileSource::new();
    let api_key = "3e607de6969c687053f9e107a4796962".to_string();
    let cache_dir = PathBuf::from("./elevation_cache");
    let api = OpenTopographySource::new(api_key, cache_dir);
    
    let mut pipeline = ElevationPipeline::new();
    if let Some(nas_source) = nas {
        println!("Using NAS SRTM file");
        pipeline.add_source(Box::new(nas_source));
    } else {
        println!("Using OpenTopography API");
    }
    pipeline.add_source(Box::new(api));
    
    let mut generator = TerrainGenerator::new(pipeline);
    let mut octree = Octree::new();
    
    // Generate 10m × 10m terrain
    println!("Generating voxel columns...");
    for lat_offset in 0..10 {
        for lon_offset in 0..10 {
            let lat = -27.4775 + (lat_offset as f64) * 0.00009;
            let lon = 153.0355 + (lon_offset as f64) * 0.00009;
            let gps = GPS::new(lat, lon, 0.0);
            
            generator.generate_column(&mut octree, &gps).unwrap();
        }
    }
    
    // Extract mesh
    println!("Extracting mesh...");
    let mut mesh = Mesh::new();
    
    for lat_offset in 0..10 {
        for lon_offset in 0..10 {
            let lat = -27.4775 + (lat_offset as f64) * 0.00009;
            let lon = 153.0355 + (lon_offset as f64) * 0.00009;
            
            for height in -10..50 {
                let ecef = GPS::new(lat, lon, height as f64).to_ecef();
                let voxel_pos = VoxelCoord::from_ecef(&ecef);
                
                // Sample 8 corners
                let mut corners = [false; 8];
                let offsets = [
                    (0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1),
                    (0, 1, 0), (1, 1, 0), (1, 1, 1), (0, 1, 1),
                ];
                
                for (i, (dx, dy, dz)) in offsets.iter().enumerate() {
                    let corner_pos = VoxelCoord::new(
                        voxel_pos.x + dx,
                        voxel_pos.y + dy,
                        voxel_pos.z + dz,
                    );
                    corners[i] = octree.get_voxel(corner_pos) != MaterialId::AIR;
                }
                
                // FIXED: Use 1m spacing, not 10m
                let cube_mesh = extract_cube_mesh(
                    Vec3::new(
                        lat_offset as f32,
                        height as f32,
                        lon_offset as f32,
                    ),
                    &corners,
                );
                
                mesh.merge(&cube_mesh);
            }
        }
    }
    
    mesh
}
