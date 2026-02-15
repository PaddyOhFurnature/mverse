use metaverse_core::renderer::{camera::Camera, pipeline::BasicPipeline, Renderer};
use metaverse_core::world_manager::WorldManager;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::cache::DiskCache;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos, EcefPos};
use metaverse_core::osm::OsmData;
use metaverse_core::mesh_generation::svo_meshes_to_colored_vertices;
use metaverse_core::materials::MaterialColors;
use wgpu::util::DeviceExt;
use std::sync::Arc;
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;
use pollster::block_on;

// Story Bridge center from REFERENCE_IMAGES.md
const CENTER_GPS: GpsPos = GpsPos {
    lat_deg: -27.463697,
    lon_deg: 153.035725,
    elevation_m: 0.0,
};

struct CameraSetup {
    filename: &'static str,
    altitude_m: f64,
    heading_deg: f64,  // 0=N, 90=E, 180=S, 270=W
    tilt_deg: f64,     // 0=down, 90=horizontal
}

const VIEWS: &[CameraSetup] = &[
    CameraSetup { filename: "01_top_down.png", altitude_m: 250.0, heading_deg: 0.0, tilt_deg: 0.0 },
    CameraSetup { filename: "02_north_horizontal.png", altitude_m: 250.0, heading_deg: 0.0, tilt_deg: 90.0 },
    CameraSetup { filename: "03_east_horizontal.png", altitude_m: 250.0, heading_deg: 90.0, tilt_deg: 90.0 },
    CameraSetup { filename: "04_south_horizontal.png", altitude_m: 250.0, heading_deg: 180.0, tilt_deg: 90.0 },
    CameraSetup { filename: "05_west_horizontal.png", altitude_m: 250.0, heading_deg: 270.0, tilt_deg: 90.0 },
    CameraSetup { filename: "06_northeast_angle.png", altitude_m: 250.0, heading_deg: 45.0, tilt_deg: 45.0 },
    CameraSetup { filename: "07_southeast_angle.png", altitude_m: 250.0, heading_deg: 135.0, tilt_deg: 45.0 },
    CameraSetup { filename: "08_southwest_angle.png", altitude_m: 250.0, heading_deg: 225.0, tilt_deg: 45.0 },
    CameraSetup { filename: "09_northwest_angle.png", altitude_m: 250.0, heading_deg: 315.0, tilt_deg: 45.0 },
    CameraSetup { filename: "10_ground_level_north.png", altitude_m: 20.0, heading_deg: 0.0, tilt_deg: 85.0 },
];

fn main() {
    println!("=== WorldManager Screenshot Capture ===\n");
    
    std::fs::create_dir_all("screenshot").ok();
    
    // Load OSM data
    println!("Loading OSM data...");
    let cache = DiskCache::new().expect("Failed to create cache");
    let osm_data = if let Ok(bytes) = cache.read_osm("brisbane_wide") {
        serde_json::from_slice::<OsmData>(&bytes).ok()
    } else {
        cache.read_osm("brisbane_cbd")
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OsmData>(&bytes).ok())
    };
    
    if osm_data.is_none() {
        eprintln!("No OSM data in cache. Run: cargo run --example download_brisbane_data");
        return;
    }
    let osm_data = Arc::new(osm_data.unwrap());
    println!("  ✓ Loaded {} buildings", osm_data.buildings.len());
    
    // Setup SRTM
    let cache_srtm = DiskCache::new().expect("Failed to create cache");
    let mut srtm = SrtmManager::new(cache_srtm);
    srtm.set_network_enabled(false);
    
    // Create window and renderer
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_inner_size(winit::dpi::PhysicalSize::new(1920, 1080))
        .with_title("Screenshot Capture")
        .build(&event_loop)
        .unwrap();
    let window = Arc::new(window);
    
    let mut renderer = block_on(Renderer::new(window.clone())).unwrap();
    let pipeline = block_on(BasicPipeline::new(&renderer.device, &renderer.config)).unwrap();
    
    // Create WorldManager
    let mut world_manager = WorldManager::new(14, 2000.0, 9);
    
    // Render each view
    for (i, view) in VIEWS.iter().enumerate() {
        println!("\n[{}/{}] Capturing: {}", i+1, VIEWS.len(), view.filename);
        
        // Calculate camera position
        let center_ecef = gps_to_ecef(&CENTER_GPS);
        let up_x = center_ecef.x / (center_ecef.x*center_ecef.x + center_ecef.y*center_ecef.y + center_ecef.z*center_ecef.z).sqrt();
        let up_y = center_ecef.y / (center_ecef.x*center_ecef.x + center_ecef.y*center_ecef.y + center_ecef.z*center_ecef.z).sqrt();
        let up_z = center_ecef.z / (center_ecef.x*center_ecef.x + center_ecef.y*center_ecef.y + center_ecef.z*center_ecef.z).sqrt();
        
        let camera_ecef = EcefPos {
            x: center_ecef.x + up_x * view.altitude_m,
            y: center_ecef.y + up_y * view.altitude_m,
            z: center_ecef.z + up_z * view.altitude_m,
        };
        
        // Convert heading/tilt to yaw/pitch
        let yaw = (90.0 - view.heading_deg).to_radians();
        let pitch = -(90.0 - view.tilt_deg).to_radians();
        
        let camera = Camera::new(
            camera_ecef,
            yaw as f32,
            pitch as f32,
            1920.0 / 1080.0,
        );
        
        // Update chunks
        println!("  Updating chunks...");
        world_manager.update(&camera_ecef, &mut srtm, &osm_data);
        
        // Extract meshes
        println!("  Extracting meshes...");
        let chunk_meshes = world_manager.extract_meshes(&camera_ecef);
        
        if chunk_meshes.is_empty() {
            println!("  ⚠ No chunks to render");
            continue;
        }
        
        // Convert to GPU format
        let material_colors = MaterialColors::default_palette();
        let mut all_vertices = Vec::new();
        let mut all_indices = Vec::new();
        
        for (meshes, chunk_center) in &chunk_meshes {
            let (verts, indices) = svo_meshes_to_colored_vertices(
                meshes,
                chunk_center,
                &camera_ecef,
                &material_colors,
            );
            
            let base_idx = all_vertices.len() as u32 / 10;
            all_vertices.extend_from_slice(&verts);
            all_indices.extend(indices.iter().map(|i| i + base_idx));
        }
        
        println!("  {} vertices, {} indices", all_vertices.len() / 10, all_indices.len());
        
        if all_vertices.is_empty() {
            println!("  ⚠ No geometry");
            continue;
        }
        
        // Upload to GPU
        let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&all_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        
        let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&all_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        
        // Update uniforms
        pipeline.update_camera(&renderer.queue, &camera, &camera_ecef);
        
        // Render
        let clear_color = wgpu::Color { r: 0.53, g: 0.81, b: 0.92, a: 1.0 };
        
        renderer.render(clear_color, |render_pass| {
            render_pass.set_pipeline(&pipeline.pipeline);
            render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..all_indices.len() as u32, 0, 0..1);
        }).ok();
        
        // Save screenshot
        println!("  Saving screenshot...");
        save_screenshot(&renderer, &format!("screenshot/{}", view.filename));
        println!("  ✓ Saved");
    }
    
    println!("\n✓ All screenshots captured!");
    println!("Compare with reference/ directory");
}

fn save_screenshot(renderer: &Renderer, path: &str) {
    // Create texture to copy surface into
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Screenshot"),
        size: wgpu::Extent3d {
            width: renderer.size.width,
            height: renderer.size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    
    // Create buffer for readback
    let bytes_per_row = renderer.size.width * 4;
    let padded_bytes_per_row = (bytes_per_row + 255) & !255;
    let buffer_size = (padded_bytes_per_row * renderer.size.height) as u64;
    
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Screenshot Buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    
    let mut encoder = renderer.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Screenshot Encoder"),
    });
    
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
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(renderer.size.height),
            },
        },
        texture.size(),
    );
    
    renderer.queue.submit(std::iter::once(encoder.finish()));
    
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        tx.send(result).unwrap();
    });
    
    renderer.device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    
    let data = slice.get_mapped_range();
    let mut pixels = vec![0u8; (bytes_per_row * renderer.size.height) as usize];
    
    for row in 0..renderer.size.height {
        let src_start = (row * padded_bytes_per_row) as usize;
        let src_end = src_start + bytes_per_row as usize;
        let dst_start = (row * bytes_per_row) as usize;
        let dst_end = dst_start + bytes_per_row as usize;
        pixels[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
    }
    
    drop(data);
    buffer.unmap();
    
    image::save_buffer(
        path,
        &pixels,
        renderer.size.width,
        renderer.size.height,
        image::ColorType::Rgba8,
    ).unwrap();
}
