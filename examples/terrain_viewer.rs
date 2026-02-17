//! Simple terrain viewer for validation
//!
//! Generates terrain at Kangaroo Point and renders it.
//! WASD to move, mouse to look, ESC to quit.

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
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
};

fn main() {
    env_logger::init();
    
    println!("=== Terrain Viewer - Validation Tool ===\n");
    
    // Create window and event loop
    let event_loop = EventLoop::new().unwrap();
    let window = event_loop
        .create_window(
            winit::window::WindowAttributes::default()
                .with_title("Metaverse - Terrain Validation")
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        )
        .unwrap();
    
    let window = Arc::new(window);
    
    // Initialize wgpu
    println!("Initializing wgpu...");
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let mut pipeline = RenderPipeline::new(&context);
    
    // Initialize camera looking at terrain
    let aspect = context.size.width as f32 / context.size.height as f32;
    let mut camera = Camera::new(
        Vec3::new(0.0, 50.0, -100.0), // 100m back, 50m up
        aspect,
    );
    
    // Generate terrain
    println!("\nGenerating terrain at Kangaroo Point...");
    let start = Instant::now();
    
    let terrain_mesh = generate_terrain_mesh();
    
    println!("Terrain generated in {:.2}s", start.elapsed().as_secs_f64());
    println!("  Vertices: {}", terrain_mesh.vertex_count());
    println!("  Triangles: {}", terrain_mesh.triangle_count());
    
    // Upload mesh to GPU
    println!("\nUploading mesh to GPU...");
    let mesh_buffer = MeshBuffer::from_mesh(&context.device, &terrain_mesh);
    
    // Event loop state
    let mut last_frame = Instant::now();
    let mut cursor_grabbed = false;
    
    println!("\n=== Controls ===");
    println!("WASD: Move camera");
    println!("Space/Shift: Up/Down");
    println!("Mouse: Look around");
    println!("ESC: Quit");
    println!("\nStarting renderer...\n");
    
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if window_id == window.id() => match event {
                WindowEvent::CloseRequested
                | WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key: PhysicalKey::Code(KeyCode::Escape),
                            ..
                        },
                    ..
                } => elwt.exit(),
                
                WindowEvent::Resized(physical_size) => {
                    context.resize(*physical_size);
                    pipeline.resize(&context.device, &context.config);
                    camera.resize(physical_size.width, physical_size.height);
                }
                
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key,
                            state,
                            ..
                        },
                    ..
                } => {
                    let dt = last_frame.elapsed().as_secs_f32();
                    camera.process_keyboard(physical_key, *state, dt);
                }
                
                WindowEvent::RedrawRequested => {
                    // Calculate delta time
                    let dt = last_frame.elapsed().as_secs_f32();
                    last_frame = Instant::now();
                    
                    // Update camera
                    pipeline.update_camera(&context.queue, &camera);
                    
                    // Render frame
                    let output = context.surface.get_current_texture().unwrap();
                    let view = output
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    
                    let mut encoder = context
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Render Encoder"),
                        });
                    
                    {
                        let mut render_pass = pipeline.begin_frame(&mut encoder, &view);
                        pipeline.set_pipeline(&mut render_pass);
                        mesh_buffer.render(&mut render_pass);
                    }
                    
                    context.queue.submit(std::iter::once(encoder.finish()));
                    output.present();
                    
                    window.request_redraw();
                }
                
                _ => {}
            },
            
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                camera.process_mouse(delta.0, delta.1);
            }
            
            _ => {}
        }
    }).unwrap();
}

/// Generate terrain mesh for Kangaroo Point
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
    
    // Generate 10m × 10m terrain (100 columns)
    println!("Generating voxel columns...");
    for lat_offset in 0..10 {
        for lon_offset in 0..10 {
            let lat = -27.4775 + (lat_offset as f64) * 0.00009; // ~10m steps
            let lon = 153.0355 + (lon_offset as f64) * 0.00009;
            let gps = GPS::new(lat, lon, 0.0);
            
            generator.generate_column(&mut octree, &gps).unwrap();
        }
    }
    
    // Extract mesh using marching cubes
    println!("Extracting mesh...");
    let mut mesh = Mesh::new();
    
    // Sample the terrain region and extract mesh cubes
    for lat_offset in 0..10 {
        for lon_offset in 0..10 {
            let lat = -27.4775 + (lat_offset as f64) * 0.00009;
            let lon = 153.0355 + (lon_offset as f64) * 0.00009;
            
            // Check voxels at different heights
            for height in -10..50 {
                let ecef = GPS::new(lat, lon, height as f64).to_ecef();
                let voxel_pos = VoxelCoord::from_ecef(&ecef);
                
                // Sample 8 corners of cube
                let mut corners = [false; 8];
                let offsets = [
                    (0, 0, 0),
                    (1, 0, 0),
                    (1, 0, 1),
                    (0, 0, 1),
                    (0, 1, 0),
                    (1, 1, 0),
                    (1, 1, 1),
                    (0, 1, 1),
                ];
                
                for (i, (dx, dy, dz)) in offsets.iter().enumerate() {
                    let corner_pos = VoxelCoord::new(
                        voxel_pos.x + dx,
                        voxel_pos.y + dy,
                        voxel_pos.z + dz,
                    );
                    let material = octree.get_voxel(corner_pos);
                    corners[i] = material != MaterialId::AIR;
                }
                
                // Extract mesh for this cube
                let cube_mesh = extract_cube_mesh(
                    Vec3::new(
                        lat_offset as f32 * 10.0,
                        height as f32,
                        lon_offset as f32 * 10.0,
                    ),
                    &corners,
                );
                
                // Merge into main mesh
                mesh.merge(&cube_mesh);
            }
        }
    }
    
    mesh
}
