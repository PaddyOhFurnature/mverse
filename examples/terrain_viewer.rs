//! Interactive terrain viewer - SAME CODE as screenshot tool but with window

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource, OpenTopographySource},
    marching_cubes::extract_octree_mesh,
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
    event::*,
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
};

fn main() {
    env_logger::init();
    
    println!("=== Terrain Viewer - 250m × 250m ===\n");
    
    // Create window
    let event_loop = EventLoop::new().unwrap();
    let window = event_loop
        .create_window(
            winit::window::WindowAttributes::default()
                .with_title("Terrain Viewer")
                .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080))
        )
        .unwrap();
    
    let window = Arc::new(window);
    
    // Initialize wgpu
    println!("Initializing wgpu...");
    let mut context = pollster::block_on(RenderContext::new(window.clone()));
    let mut pipeline = RenderPipeline::new(&context);
    
    // Generate terrain (EXACT SAME as screenshot tool)
    println!("\nGenerating 250m × 250m terrain...");
    println!("(Good balance: 10s load, 1.5M vertices, 60 FPS)");
    let start = Instant::now();
    
    let nas = NasFileSource::new();
    let api_key = "3e607de6969c687053f9e107a4796962".to_string();
    let cache_dir = PathBuf::from("./elevation_cache");
    let api = OpenTopographySource::new(api_key, cache_dir);
    
    let mut elevation_pipeline = ElevationPipeline::new();
    if let Some(nas_source) = nas {
        println!("Using NAS SRTM file");
        elevation_pipeline.add_source(Box::new(nas_source));
    } else {
        println!("Using OpenTopography API");
    }
    elevation_pipeline.add_source(Box::new(api));
    
    let mut generator = TerrainGenerator::new(elevation_pipeline);
    let mut octree = Octree::new();
    
    let origin = GPS::new(-27.4775, 153.0355, 0.0);
    generator.generate_region(&mut octree, &origin, 250.0)
        .expect("Failed to generate terrain");
    
    let origin_ecef = origin.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    
    let terrain_mesh = extract_octree_mesh(&octree, &origin_voxel, 8);
    
    println!("Generated in {:.2}s", start.elapsed().as_secs_f32());
    println!("  Vertices: {}", terrain_mesh.vertex_count());
    println!("  Triangles: {}", terrain_mesh.triangle_count());
    
    if terrain_mesh.is_empty() {
        eprintln!("\nERROR: No mesh generated!");
        return;
    }
    
    // Upload to GPU
    println!("\nUploading to GPU...");
    let mesh_buffer = MeshBuffer::from_mesh(&context.device, &terrain_mesh);
    
    // Setup camera (SAME as screenshot)
    let aspect = context.size.width as f32 / context.size.height as f32;
    let mut camera = Camera::new(Vec3::new(-80.0, 60.0, 80.0), aspect);
    camera.yaw = (-45.0_f32).to_radians();
    camera.pitch = (-25.0_f32).to_radians();
    
    let mut last_frame = Instant::now();
    
    println!("\n=== Controls ===");
    println!("WASD: Move | Mouse: Look | T: Reset camera | P: Print position | ESC: Quit\n");
    
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { ref event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested
                | WindowEvent::KeyboardInput {
                    event: KeyEvent {
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
                    event: KeyEvent { physical_key, state, .. },
                    ..
                } => {
                    let dt = last_frame.elapsed().as_secs_f32();
                    
                    if *state == ElementState::Pressed {
                        if let PhysicalKey::Code(code) = physical_key {
                            match code {
                                KeyCode::KeyT => {
                                    camera.position = Vec3::new(-80.0, 60.0, 80.0);
                                    camera.yaw = (-45.0_f32).to_radians();
                                    camera.pitch = (-25.0_f32).to_radians();
                                    println!("Camera reset to overview");
                                }
                                KeyCode::KeyP => {
                                    println!("Position: {:?}", camera.position);
                                    println!("Yaw: {:.1}° Pitch: {:.1}°", 
                                        camera.yaw.to_degrees(), camera.pitch.to_degrees());
                                }
                                _ => {}
                            }
                        }
                    }
                    
                    camera.process_keyboard(physical_key, *state, dt);
                }
                
                WindowEvent::RedrawRequested => {
                    last_frame = Instant::now();
                    
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
