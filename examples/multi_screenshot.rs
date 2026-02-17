//! Multi-angle screenshot tool - captures terrain from 5 different viewpoints

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource, OpenTopographySource},
    marching_cubes::extract_octree_mesh,
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
    
    println!("=== Multi-Angle Screenshot Tool ===\n");
    
    // Create window
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let window = event_loop.create_window(
        winit::window::WindowAttributes::default()
            .with_title("Screenshot")
            .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080))
            .with_visible(false)
    ).unwrap();
    let window = Arc::new(window);
    
    // Initialize wgpu
    println!("Initializing wgpu...");
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let mut pipeline = RenderPipeline::new(&context);
    
    // Generate terrain
    println!("Generating terrain...");
    let start = Instant::now();
    
    let nas = NasFileSource::new();
    let mut elevation_pipeline = ElevationPipeline::new();
    if let Some(nas_source) = nas {
        elevation_pipeline.add_source(Box::new(nas_source));
    }
    
    let mut generator = TerrainGenerator::new(elevation_pipeline);
    let mut octree = Octree::new();
    let origin = GPS::new(-27.4775, 153.0355, 0.0);
    
    generator.generate_region(&mut octree, &origin, 120.0).unwrap();
    
    let origin_ecef = origin.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    let mesh = extract_octree_mesh(&octree, &origin_voxel, 7);
    
    println!("Generated in {:.2}s - {} vertices", start.elapsed().as_secs_f32(), mesh.vertex_count());
    
    let mesh_buffer = MeshBuffer::from_mesh(&context.device, &mesh);
    
    // Camera positions
    let positions = vec![
        ("overview", Vec3::new(-100.0, 80.0, 100.0), (-45.0_f32).to_radians(), (-30.0_f32).to_radians()),
        ("aerial", Vec3::new(0.0, 150.0, 0.0), 0.0, (-85.0_f32).to_radians()),
        ("ground_north", Vec3::new(0.0, 5.0, 80.0), (-90.0_f32).to_radians(), 0.0),
        ("ground_east", Vec3::new(80.0, 5.0, 0.0), (180.0_f32).to_radians(), 0.0),
        ("close_angled", Vec3::new(-60.0, 40.0, 60.0), (-45.0_f32).to_radians(), (-20.0_f32).to_radians()),
    ];
    
    std::fs::create_dir_all("screenshot").ok();
    
    for (name, pos, yaw, pitch) in &positions {
        println!("\nCapturing: {}", name);
        
        let aspect = 1920.0 / 1080.0;
        let mut camera = Camera::new(*pos, aspect);
        camera.yaw = *yaw;
        camera.pitch = *pitch;
        
        pipeline.update_camera(&context.queue, &camera);
        
        // Create offscreen texture
        let texture = context.device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d { width: 1920, height: 1080, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: context.surface.get_capabilities(&context.adapter).formats[0],
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
            label: None,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        let depth_texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth"),
            size: wgpu::Extent3d { width: 1920, height: 1080, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Render
        let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.5, g: 0.7, b: 0.9, a: 1.0 }),
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
        let padded_bytes_per_row = ((1920 * 4) + 255) & !255;
        let output_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
            size: (padded_bytes_per_row * 1080) as u64,
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
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(1080),
                },
            },
            wgpu::Extent3d { width: 1920, height: 1080, depth_or_array_layers: 1 },
        );
        
        context.queue.submit(Some(encoder.finish()));
        
        // Save PNG
        let buffer_slice = output_buffer.slice(..);
        buffer_slice.map_async(wgpu::MapMode::Read, |_| {});
        context.device.poll(wgpu::Maintain::Wait);
        
        let data = buffer_slice.get_mapped_range();
        let mut png_data = Vec::with_capacity((1920 * 1080 * 4) as usize);
        for chunk in data.chunks(padded_bytes_per_row as usize) {
            png_data.extend_from_slice(&chunk[..(1920 * 4) as usize]);
        }
        
        let path = PathBuf::from(format!("screenshot/terrain_{}.png", name));
        image::save_buffer(&path, &png_data, 1920, 1080, image::ColorType::Rgba8).unwrap();
        println!("  ✓ Saved: {}", path.display());
    }
    
    println!("\n✓ All screenshots captured!");
}
