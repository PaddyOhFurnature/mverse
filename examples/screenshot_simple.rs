//! Simple screenshot tool - captures one image using WorldManager

use metaverse_core::renderer::{camera::Camera, pipeline::BasicPipeline, Renderer};
use metaverse_core::world_manager::WorldManager;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::cache::DiskCache;
use metaverse_core::osm::OsmData;
use metaverse_core::mesh_generation::svo_meshes_to_colored_vertices;
use metaverse_core::materials::MaterialColors;
use metaverse_core::coordinates::EcefPos;
use wgpu::util::DeviceExt;
use std::sync::Arc;
use winit::event_loop::EventLoop;
use winit::window::WindowAttributes;

fn main() {
    println!("=== Simple Screenshot Test ===\n");
    
    // Load data
    println!("Loading OSM data...");
    let cache = DiskCache::new().expect("Cache");
    let osm_data = cache.read_osm("brisbane_wide")
        .or_else(|_| cache.read_osm("brisbane_cbd"))
        .and_then(|bytes| serde_json::from_slice::<OsmData>(&bytes))
        .expect("No OSM data - run download_brisbane_data");
    let osm_data = Arc::new(osm_data);
    println!("  {} buildings", osm_data.buildings.len());
    
    let cache_srtm = DiskCache::new().expect("Cache");
    let mut srtm = SrtmManager::new(cache_srtm);
    srtm.set_network_enabled(false);
    
    // Create window
    let event_loop = EventLoop::new().unwrap();
    let window_attrs = WindowAttributes::default()
        .with_inner_size(winit::dpi::PhysicalSize::new(1920, 1080))
        .with_title("Screenshot");
    let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
    
    // Create renderer
    let renderer = pollster::block_on(async {
        Renderer::new(window.clone()).await
    });
    
    let pipeline = BasicPipeline::new(&renderer.device, &renderer.config);
    
    // Create WorldManager
    let mut world_manager = WorldManager::new(14, 2000.0, 9);
    
    // Use camera from viewer
    let camera = Camera::brisbane();
    let camera_ecef = EcefPos {
        x: camera.position.x,
        y: camera.position.y,
        z: camera.position.z,
    };
    
    // Update chunks
    println!("\nGenerating chunks...");
    world_manager.update(&camera_ecef, &mut srtm, &osm_data);
    
    // Extract meshes
    println!("Extracting meshes...");
    let chunk_meshes = world_manager.extract_meshes(&camera_ecef);
    println!("  {} chunks", chunk_meshes.len());
    
    // Convert to GPU format
    let material_colors = MaterialColors::default_palette();
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    for (meshes, chunk_center) in &chunk_meshes {
        let (verts, indices) = svo_meshes_to_colored_vertices(meshes, &material_colors);
        
        // Transform from chunk-local to ECEF (relative to camera)
        let offset_x = (chunk_center.x - camera_ecef.x) as f32;
        let offset_y = (chunk_center.y - camera_ecef.y) as f32;
        let offset_z = (chunk_center.z - camera_ecef.z) as f32;
        
        let base_idx = all_vertices.len() as u32 / 10;
        
        for i in (0..verts.len()).step_by(10) {
            all_vertices.push(verts[i] + offset_x);
            all_vertices.push(verts[i+1] + offset_y);
            all_vertices.push(verts[i+2] + offset_z);
            all_vertices.extend_from_slice(&verts[i+3..i+10]);
        }
        
        all_indices.extend(indices.iter().map(|idx| idx + base_idx));
    }
    
    println!("  {} vertices", all_vertices.len() / 10);
    
    if all_vertices.is_empty() {
        println!("No geometry!");
        return;
    }
    
    // Upload to GPU
    let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Vertex"),
        contents: bytemuck::cast_slice(&all_vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    
    let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Index"),
        contents: bytemuck::cast_slice(&all_indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    
    // Update camera uniforms
    pipeline.update_camera(&renderer.queue, &camera, &camera_ecef);
    
    // Render
    println!("Rendering...");
    let clear = wgpu::Color { r: 0.53, g: 0.81, b: 0.92, a: 1.0 };
    
    renderer.render(clear, |pass| {
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..all_indices.len() as u32, 0, 0..1);
    }).ok();
    
    println!("✓ Done - check viewer window");
    std::thread::sleep(std::time::Duration::from_secs(5));
}
