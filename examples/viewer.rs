//! Metaverse viewer
//!
//! Opens a window and renders the metaverse world.

use metaverse_core::renderer::{
    camera::Camera, 
    pipeline::{BasicPipeline, Vertex},
    mesh::{generate_earth_sphere, generate_tile_outlines, generate_terrain_patches, generate_buildings_from_osm},
    Renderer
};
use metaverse_core::osm::{load_chunk_osm_data, OverpassClient};
use metaverse_core::chunks::ChunkId;
use metaverse_core::cache::DiskCache;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::*;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};
use wgpu::util::DeviceExt;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pipeline: Option<BasicPipeline>,
    line_pipeline: Option<BasicPipeline>,
    camera: Camera,
    sphere_vertex_buffer: Option<wgpu::Buffer>,
    sphere_index_buffer: Option<wgpu::Buffer>,
    sphere_num_indices: u32,
    tile_vertex_buffer: Option<wgpu::Buffer>,
    tile_index_buffer: Option<wgpu::Buffer>,
    tile_num_indices: u32,
    terrain_vertex_buffer: Option<wgpu::Buffer>,
    terrain_index_buffer: Option<wgpu::Buffer>,
    terrain_num_indices: u32,
    buildings_vertex_buffer: Option<wgpu::Buffer>,
    buildings_index_buffer: Option<wgpu::Buffer>,
    buildings_num_indices: u32,
    tile_depth: u8,
    show_sphere: bool,
    show_tiles: bool,
    show_terrain: bool,
    show_buildings: bool,
    frame_count: usize,
    fps_update_time: std::time::Instant,
    last_frame_time: std::time::Instant,
    
    // Input state
    keys_pressed: std::collections::HashSet<KeyCode>,
    mouse_captured: bool,
    last_mouse_pos: Option<(f64, f64)>,
}

impl App {
    fn new() -> Self {
        // Start camera at Brisbane looking down at city
        let mut camera = Camera::brisbane();
        
        // Move camera to 5km altitude for better city view
        camera.position = camera.position.normalize() * (6_371_000.0 + 5000.0);
        
        // Look down at Brisbane
        camera.orientation = {
            use glam::{DVec3, DQuat, DMat3};
            let forward = -camera.position.normalize(); // Look toward Earth center
            let up = DVec3::Z;
            let right = forward.cross(up).normalize();
            let up = right.cross(forward).normalize();
            let rotation_matrix = DMat3::from_cols(right, up, -forward);
            DQuat::from_mat3(&rotation_matrix)
        };
        
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            line_pipeline: None,
            camera,
            sphere_vertex_buffer: None,
            sphere_index_buffer: None,
            sphere_num_indices: 0,
            tile_vertex_buffer: None,
            tile_index_buffer: None,
            tile_num_indices: 0,
            terrain_vertex_buffer: None,
            terrain_index_buffer: None,
            terrain_num_indices: 0,
            buildings_vertex_buffer: None,
            buildings_index_buffer: None,
            buildings_num_indices: 0,
            tile_depth: 2, // Start with depth 2 (96 tiles)
            show_sphere: false, // Hide sphere by default
            show_tiles: false, // Hide tiles by default
            show_terrain: true,
            show_buildings: true,
            frame_count: 0,
            fps_update_time: std::time::Instant::now(),
            last_frame_time: std::time::Instant::now(),
            keys_pressed: std::collections::HashSet::new(),
            mouse_captured: false,
            last_mouse_pos: None,
        }
    }
    
    fn handle_input(&mut self, delta_time: f64) {
        // Movement: WASD + QE for up/down
        let mut forward = 0.0;
        let mut right = 0.0;
        let mut up = 0.0;
        
        if self.keys_pressed.contains(&KeyCode::KeyW) {
            forward += 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyS) {
            forward -= 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyD) {
            right += 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyA) {
            right -= 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyE) || self.keys_pressed.contains(&KeyCode::Space) {
            up += 1.0;
        }
        if self.keys_pressed.contains(&KeyCode::KeyQ) || self.keys_pressed.contains(&KeyCode::ShiftLeft) {
            up -= 1.0;
        }
        
        // Speed modifiers
        let mut speed_mod = 1.0;
        if self.keys_pressed.contains(&KeyCode::ControlLeft) {
            speed_mod *= 0.1; // Slow
        }
        if self.keys_pressed.contains(&KeyCode::ShiftRight) {
            speed_mod *= 10.0; // Fast
        }
        
        let original_multiplier = self.camera.speed_multiplier;
        self.camera.speed_multiplier *= speed_mod;
        
        if forward != 0.0 || right != 0.0 || up != 0.0 {
            self.camera.move_relative(forward, right, up, delta_time);
        }
        
        self.camera.speed_multiplier = original_multiplier;
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = Arc::new(
                event_loop
                    .create_window(
                        WindowAttributes::default()
                            .with_title("Metaverse Viewer")
                            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                    )
                    .unwrap(),
            );

            // Create renderer
            let renderer = pollster::block_on(Renderer::new(window.clone()));
            
            // Create pipelines
            let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
            let line_pipeline = BasicPipeline::new_with_topology(
                &renderer.device, 
                renderer.config.format, 
                wgpu::PrimitiveTopology::LineList
            );
            
            // Generate Earth sphere
            let (vertices, indices) = generate_earth_sphere();
            let sphere_num_indices = indices.len() as u32;
            
            let sphere_vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Earth Sphere Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            
            let sphere_index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Earth Sphere Index Buffer"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            
            self.sphere_vertex_buffer = Some(sphere_vertex_buffer);
            self.sphere_index_buffer = Some(sphere_index_buffer);
            self.sphere_num_indices = sphere_num_indices;
            self.pipeline = Some(pipeline);
            self.line_pipeline = Some(line_pipeline);
            
            // Generate initial tiles and terrain
            let (tile_vertices, tile_indices) = generate_tile_outlines(self.tile_depth, None);
            let tile_num_indices = tile_indices.len() as u32;
            
            let tile_vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Tile Outline Vertex Buffer"),
                contents: bytemuck::cast_slice(&tile_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            
            let tile_index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Tile Outline Index Buffer"),
                contents: bytemuck::cast_slice(&tile_indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            
            // Generate terrain patches (green)
            let (terrain_vertices, terrain_indices) = generate_terrain_patches(
                self.tile_depth, 
                16, // 16x16 subdivisions
                glam::Vec3::new(0.2, 0.8, 0.2) // Green
            );
            let terrain_num_indices = terrain_indices.len() as u32;
            
            let terrain_vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Terrain Patch Vertex Buffer"),
                contents: bytemuck::cast_slice(&terrain_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            
            let terrain_index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Terrain Patch Index Buffer"),
                contents: bytemuck::cast_slice(&terrain_indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            
            self.tile_vertex_buffer = Some(tile_vertex_buffer);
            self.tile_index_buffer = Some(tile_index_buffer);
            self.tile_num_indices = tile_num_indices;
            self.terrain_vertex_buffer = Some(terrain_vertex_buffer);
            self.terrain_index_buffer = Some(terrain_index_buffer);
            self.terrain_num_indices = terrain_num_indices;
            
            // Load OSM buildings for Brisbane CBD
            println!("Loading OSM buildings for Brisbane CBD...");
            let cache = DiskCache::new().unwrap();
            let client = OverpassClient::new(2); // 2 second cooldown
            
            // Brisbane CBD is roughly at chunk depth 14
            let brisbane_chunk = ChunkId {
                face: 0,
                path: vec![0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1], // Approx Brisbane location
            };
            
            let buildings_color = glam::Vec3::new(0.7, 0.7, 0.8); // Light gray/blue
            let (buildings_vertices, buildings_indices) = match load_chunk_osm_data(&brisbane_chunk, 14, &client, &cache) {
                Ok(osm_data) => {
                    println!("Loaded {} buildings from OSM", osm_data.buildings.len());
                    generate_buildings_from_osm(&osm_data, buildings_color)
                }
                Err(e) => {
                    println!("Failed to load OSM data: {}, using empty", e);
                    (Vec::new(), Vec::new())
                }
            };
            
            let buildings_num_indices = buildings_indices.len() as u32;
            
            if !buildings_vertices.is_empty() {
                let buildings_vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Buildings Vertex Buffer"),
                    contents: bytemuck::cast_slice(&buildings_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                
                let buildings_index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Buildings Index Buffer"),
                    contents: bytemuck::cast_slice(&buildings_indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                
                self.buildings_vertex_buffer = Some(buildings_vertex_buffer);
                self.buildings_index_buffer = Some(buildings_index_buffer);
                self.buildings_num_indices = buildings_num_indices;
                
                println!("Generated {} building vertices, {} indices", buildings_vertices.len(), buildings_indices.len());
            } else {
                println!("No buildings to render");
            }
            
            self.renderer = Some(renderer);
            self.window = Some(window);
            self.fps_update_time = std::time::Instant::now();
            self.last_frame_time = std::time::Instant::now();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(physical_size);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(keycode) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            self.keys_pressed.insert(keycode);
                            
                            // Tile depth controls
                            if keycode == KeyCode::BracketLeft && self.tile_depth > 0 {
                                self.tile_depth -= 1;
                                if let Some(renderer) = &self.renderer {
                                    // Regenerate tiles
                                    let (vertices, indices) = generate_tile_outlines(self.tile_depth, None);
                                    let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                        label: Some("Tile Outline Vertex Buffer"),
                                        contents: bytemuck::cast_slice(&vertices),
                                        usage: wgpu::BufferUsages::VERTEX,
                                    });
                                    let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                        label: Some("Tile Outline Index Buffer"),
                                        contents: bytemuck::cast_slice(&indices),
                                        usage: wgpu::BufferUsages::INDEX,
                                    });
                                    self.tile_vertex_buffer = Some(vertex_buffer);
                                    self.tile_index_buffer = Some(index_buffer);
                                    self.tile_num_indices = indices.len() as u32;
                                    
                                    // Regenerate terrain
                                    let (terrain_verts, terrain_inds) = generate_terrain_patches(
                                        self.tile_depth, 16, glam::Vec3::new(0.2, 0.8, 0.2)
                                    );
                                    let terrain_vb = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                        label: Some("Terrain Patch Vertex Buffer"),
                                        contents: bytemuck::cast_slice(&terrain_verts),
                                        usage: wgpu::BufferUsages::VERTEX,
                                    });
                                    let terrain_ib = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                        label: Some("Terrain Patch Index Buffer"),
                                        contents: bytemuck::cast_slice(&terrain_inds),
                                        usage: wgpu::BufferUsages::INDEX,
                                    });
                                    self.terrain_vertex_buffer = Some(terrain_vb);
                                    self.terrain_index_buffer = Some(terrain_ib);
                                    self.terrain_num_indices = terrain_inds.len() as u32;
                                    
                                    println!("Generated tiles at depth {} ({} tiles)", 
                                        self.tile_depth, 6 * 4_u32.pow(self.tile_depth as u32));
                                }
                            } else if keycode == KeyCode::BracketRight && self.tile_depth < 5 {
                                self.tile_depth += 1;
                                if let Some(renderer) = &self.renderer {
                                    // Regenerate tiles
                                    let (vertices, indices) = generate_tile_outlines(self.tile_depth, None);
                                    let vertex_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                        label: Some("Tile Outline Vertex Buffer"),
                                        contents: bytemuck::cast_slice(&vertices),
                                        usage: wgpu::BufferUsages::VERTEX,
                                    });
                                    let index_buffer = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                        label: Some("Tile Outline Index Buffer"),
                                        contents: bytemuck::cast_slice(&indices),
                                        usage: wgpu::BufferUsages::INDEX,
                                    });
                                    self.tile_vertex_buffer = Some(vertex_buffer);
                                    self.tile_index_buffer = Some(index_buffer);
                                    self.tile_num_indices = indices.len() as u32;
                                    
                                    // Regenerate terrain
                                    let (terrain_verts, terrain_inds) = generate_terrain_patches(
                                        self.tile_depth, 16, glam::Vec3::new(0.2, 0.8, 0.2)
                                    );
                                    let terrain_vb = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                        label: Some("Terrain Patch Vertex Buffer"),
                                        contents: bytemuck::cast_slice(&terrain_verts),
                                        usage: wgpu::BufferUsages::VERTEX,
                                    });
                                    let terrain_ib = renderer.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                        label: Some("Terrain Patch Index Buffer"),
                                        contents: bytemuck::cast_slice(&terrain_inds),
                                        usage: wgpu::BufferUsages::INDEX,
                                    });
                                    self.terrain_vertex_buffer = Some(terrain_vb);
                                    self.terrain_index_buffer = Some(terrain_ib);
                                    self.terrain_num_indices = terrain_inds.len() as u32;
                                    
                                    println!("Generated tiles at depth {} ({} tiles)", 
                                        self.tile_depth, 6 * 4_u32.pow(self.tile_depth as u32));
                                }
                            }
                            
                            // Toggle visibility
                            if keycode == KeyCode::Digit1 {
                                self.show_sphere = !self.show_sphere;
                                println!("Sphere: {}", if self.show_sphere { "ON" } else { "OFF" });
                            } else if keycode == KeyCode::Digit2 {
                                self.show_tiles = !self.show_tiles;
                                println!("Tiles: {}", if self.show_tiles { "ON" } else { "OFF" });
                            } else if keycode == KeyCode::Digit3 {
                                self.show_terrain = !self.show_terrain;
                                println!("Terrain: {}", if self.show_terrain { "ON" } else { "OFF" });
                            } else if keycode == KeyCode::Digit4 {
                                self.show_buildings = !self.show_buildings;
                                println!("Buildings: {}", if self.show_buildings { "ON" } else { "OFF" });
                            }
                            
                            // Toggle mouse capture on click
                            if keycode == KeyCode::Escape {
                                self.mouse_captured = false;
                                if let Some(window) = &self.window {
                                    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                                    window.set_cursor_visible(true);
                                }
                            }
                        }
                        ElementState::Released => {
                            self.keys_pressed.remove(&keycode);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                // Capture mouse on left click
                if !self.mouse_captured {
                    self.mouse_captured = true;
                    if let Some(window) = &self.window {
                        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                        window.set_cursor_visible(false);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.mouse_captured {
                    if let Some(last_pos) = self.last_mouse_pos {
                        let delta_x = position.x - last_pos.0;
                        let delta_y = position.y - last_pos.1;
                        
                        // Mouse sensitivity
                        let sensitivity = 0.002;
                        let yaw_delta = -delta_x * sensitivity;
                        let pitch_delta = -delta_y * sensitivity;
                        
                        self.camera.rotate(pitch_delta, yaw_delta);
                    }
                    self.last_mouse_pos = Some((position.x, position.y));
                } else {
                    self.last_mouse_pos = None;
                }
            }
            WindowEvent::RedrawRequested => {
                // Calculate delta time
                let now = std::time::Instant::now();
                let delta_time = now.duration_since(self.last_frame_time).as_secs_f64();
                self.last_frame_time = now;
                
                // Handle input
                self.handle_input(delta_time);
                
                if let (Some(renderer), Some(window), Some(pipeline), Some(line_pipeline), Some(sphere_vb), Some(sphere_ib), Some(tile_vb), Some(tile_ib), Some(terrain_vb), Some(terrain_ib)) = 
                    (&mut self.renderer, &self.window, &self.pipeline, &self.line_pipeline, &self.sphere_vertex_buffer, &self.sphere_index_buffer, &self.tile_vertex_buffer, &self.tile_index_buffer, &self.terrain_vertex_buffer, &self.terrain_index_buffer) {
                    
                    // Update camera matrix with floating origin
                    let aspect = renderer.size.width as f32 / renderer.size.height as f32;
                    let (view_proj, camera_offset) = self.camera.view_projection_matrix(aspect);
                    
                    // Apply floating origin: translate sphere by -camera_offset before rendering
                    let camera_offset_f32 = glam::Vec3::new(
                        camera_offset.x as f32,
                        camera_offset.y as f32,
                        camera_offset.z as f32,
                    );
                    let origin_transform = glam::Mat4::from_translation(-camera_offset_f32);
                    let final_view_proj = view_proj * origin_transform;
                    
                    pipeline.update_uniforms(&renderer.queue, final_view_proj);
                    line_pipeline.update_uniforms(&renderer.queue, final_view_proj);
                    
                    // Sky blue color
                    let clear_color = wgpu::Color {
                        r: 0.529,
                        g: 0.808,
                        b: 0.922,
                        a: 1.0,
                    };

                    // Render frame with conditional geometry
                    let sphere_num_indices = self.sphere_num_indices;
                    let tile_num_indices = self.tile_num_indices;
                    let terrain_num_indices = self.terrain_num_indices;
                    let buildings_num_indices = self.buildings_num_indices;
                    let show_sphere = self.show_sphere;
                    let show_tiles = self.show_tiles;
                    let show_terrain = self.show_terrain;
                    let show_buildings = self.show_buildings;
                    let buildings_vb = self.buildings_vertex_buffer.as_ref();
                    let buildings_ib = self.buildings_index_buffer.as_ref();
                    
                    let result = renderer.render(clear_color, |render_pass| {
                        // Draw terrain patches first (behind everything)
                        if show_terrain {
                            render_pass.set_pipeline(&pipeline.pipeline);
                            render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                            render_pass.set_vertex_buffer(0, terrain_vb.slice(..));
                            render_pass.set_index_buffer(terrain_ib.slice(..), wgpu::IndexFormat::Uint32);
                            render_pass.draw_indexed(0..terrain_num_indices, 0, 0..1);
                        }
                        
                        // Draw buildings
                        if show_buildings {
                            if let (Some(bvb), Some(bib)) = (buildings_vb, buildings_ib) {
                                render_pass.set_pipeline(&pipeline.pipeline);
                                render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                                render_pass.set_vertex_buffer(0, bvb.slice(..));
                                render_pass.set_index_buffer(bib.slice(..), wgpu::IndexFormat::Uint32);
                                render_pass.draw_indexed(0..buildings_num_indices, 0, 0..1);
                            }
                        }
                        
                        // Draw sphere
                        if show_sphere {
                            render_pass.set_pipeline(&pipeline.pipeline);
                            render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                            render_pass.set_vertex_buffer(0, sphere_vb.slice(..));
                            render_pass.set_index_buffer(sphere_ib.slice(..), wgpu::IndexFormat::Uint32);
                            render_pass.draw_indexed(0..sphere_num_indices, 0, 0..1);
                        }
                        
                        // Draw tile outlines (on top)
                        if show_tiles {
                            render_pass.set_pipeline(&line_pipeline.pipeline);
                            render_pass.set_bind_group(0, &line_pipeline.uniform_bind_group, &[]);
                            render_pass.set_vertex_buffer(0, tile_vb.slice(..));
                            render_pass.set_index_buffer(tile_ib.slice(..), wgpu::IndexFormat::Uint32);
                            render_pass.draw_indexed(0..tile_num_indices, 0, 0..1);
                        }
                    });

                    match result {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => renderer.resize(renderer.size),
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => eprintln!("Render error: {:?}", e),
                    }

                    // Update FPS counter
                    self.frame_count += 1;
                    if now.duration_since(self.fps_update_time).as_secs_f32() >= 1.0 {
                        let fps =
                            self.frame_count as f32 / now.duration_since(self.fps_update_time).as_secs_f32();
                        
                        // Also show camera position and tile depth
                        let pos = self.camera.position;
                        let alt = pos.length() - 6_371_000.0;
                        window.set_title(&format!(
                            "Metaverse Viewer - {:.1} FPS | Alt: {:.0}m | Speed: {:.1}x | Tiles: Depth {}",
                            fps, alt, self.camera.speed_multiplier, self.tile_depth
                        ));
                        
                        self.frame_count = 0;
                        self.fps_update_time = now;
                    }

                    // Request next frame
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    env_logger::init();

    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();

    println!("=== Metaverse Viewer ===");
    println!("Controls:");
    println!("  WASD - Move forward/left/back/right");
    println!("  Q/E or Shift/Space - Move down/up");
    println!("  Right Shift - 10x speed boost");
    println!("  Left Ctrl - 0.1x speed (slow)");
    println!("  [ ] - Decrease/increase tile depth (0-5)");
    println!("  1 - Toggle sphere visibility");
    println!("  2 - Toggle tile outlines");
    println!("  3 - Toggle terrain patches");
    println!("  4 - Toggle OSM buildings");
    println!("  Escape - Release mouse");
    println!();

    event_loop.run_app(&mut app).unwrap();
}
