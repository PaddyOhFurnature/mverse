//! Continuous Query Viewer with ASYNC mesh generation
//! Fixed: No blocking, smooth 60fps movement

use metaverse_core::renderer::{Renderer, pipeline::BasicPipeline};
use metaverse_core::renderer::greedy_mesh::greedy_mesh_block;
use metaverse_core::renderer::skybox::SkyboxPipeline;
use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::renderer::camera::Camera;
use metaverse_core::renderer::pipeline::Vertex;
use metaverse_core::coordinates::{gps_to_ecef, ecef_to_gps, GpsPos, EcefPos};
use metaverse_core::svo::{AIR};
use std::sync::{Arc, Mutex, RwLock};
use winit::application::ApplicationHandler;
use winit::event::*;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};
use wgpu::util::DeviceExt;
use wgpu;

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

#[derive(Debug, Clone)]
enum MeshStatus {
    Idle,
    Generating { position: [f64; 3], started_at: std::time::Instant },
    Ready { vertices: Vec<Vertex>, indices: Vec<u32>, origin: [f64; 3] },
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pipeline: Option<BasicPipeline>,
    skybox: Option<SkyboxPipeline>,
    camera: Camera,
    continuous_world: Option<Arc<RwLock<ContinuousWorld>>>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    mesh_origin: [f64; 3],  // Position mesh was generated at
    frame_count: usize,
    fps_update_time: std::time::Instant,
    last_frame_time: std::time::Instant,
    last_mesh_request: std::time::Instant,
    last_mesh_camera_pos: [f64; 3],
    keys_pressed: std::collections::HashSet<KeyCode>,
    mouse_captured: bool,
    last_mouse_pos: Option<(f64, f64)>,
    mesh_status: Arc<Mutex<MeshStatus>>,
}

impl App {
    fn new(gps: GpsPos, pitch_deg: f64) -> Self {
        let position_ecef = gps_to_ecef(&gps);
        let position = glam::DVec3::new(position_ecef.x, position_ecef.y, position_ecef.z);
        let look_at = glam::DVec3::ZERO;
        
        let mut camera = Camera::new(position, look_at);
        if pitch_deg != 0.0 {
            camera.rotate(pitch_deg.to_radians(), 0.0);
        }
        
        println!("Camera at GPS: ({:.6}°, {:.6}°, {:.0}m)", gps.lat_deg, gps.lon_deg, gps.elevation_m);
        
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            skybox: None,
            camera,
            continuous_world: None,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            mesh_origin: [position.x, position.y, position.z],
            frame_count: 0,
            fps_update_time: std::time::Instant::now(),
            last_frame_time: std::time::Instant::now(),
            last_mesh_request: std::time::Instant::now(),
            last_mesh_camera_pos: [position.x, position.y, position.z],
            keys_pressed: std::collections::HashSet::new(),
            mouse_captured: false,
            last_mouse_pos: None,
            mesh_status: Arc::new(Mutex::new(MeshStatus::Idle)),
        }
    }
    
    fn handle_input(&mut self, delta_time: f64) {
        let mut forward = 0.0;
        let mut right = 0.0;
        let mut up = 0.0;
        
        if self.keys_pressed.contains(&KeyCode::KeyW) { forward += 1.0; }
        if self.keys_pressed.contains(&KeyCode::KeyS) { forward -= 1.0; }
        if self.keys_pressed.contains(&KeyCode::KeyD) { right += 1.0; }
        if self.keys_pressed.contains(&KeyCode::KeyA) { right -= 1.0; }
        if self.keys_pressed.contains(&KeyCode::Space) { up += 1.0; }
        if self.keys_pressed.contains(&KeyCode::ShiftLeft) { up -= 1.0; }
        
        // Debug movement
        if forward != 0.0 || right != 0.0 || up != 0.0 {
            let pos_before = self.camera.position;
            self.camera.move_relative(forward, right, up, delta_time);
            let pos_after = self.camera.position;
            let dist = (pos_after - pos_before).length();
            println!("[Movement] delta_time={:.4}s, moved={:.2}m", delta_time, dist);
        } else {
            self.camera.move_relative(forward, right, up, delta_time);
        }
    }
    
    fn request_mesh_update(&self, camera_pos: [f64; 3]) {
        let status = self.mesh_status.lock().unwrap();
        
        // Don't spawn if already generating
        if matches!(*status, MeshStatus::Generating { .. }) {
            drop(status);
            return;
        }
        drop(status);
        
        let world = Arc::clone(self.continuous_world.as_ref().unwrap());
        let mesh_status = Arc::clone(&self.mesh_status);
        
        *mesh_status.lock().unwrap() = MeshStatus::Generating {
            position: camera_pos,
            started_at: std::time::Instant::now(),
        };
        
        println!("[Async] Starting mesh generation...");
        
        // Spawn background thread
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            
            // Generate mesh (doesn't block rendering!)
            let (vertices, indices) = generate_mesh(&world, camera_pos);
            
            let elapsed = start.elapsed().as_secs_f64();
            println!("[Async] Generated {} vertices in {:.2}s", vertices.len(), elapsed);
            
            *mesh_status.lock().unwrap() = MeshStatus::Ready { 
                vertices, 
                indices,
                origin: camera_pos,
            };
        });
    }
    
    fn check_and_upload_mesh(&mut self, _current_camera_pos: [f64; 3]) {
        let mut status = self.mesh_status.lock().unwrap();
        
        if let MeshStatus::Ready { vertices, indices, origin } = &*status {
            let start = std::time::Instant::now();
            
            // Update mesh origin
            self.mesh_origin = *origin;
            
            println!("[Upload] {} vertices to GPU (origin: {:.1}, {:.1}, {:.1})", 
                     vertices.len(), self.mesh_origin[0], self.mesh_origin[1], self.mesh_origin[2]);
            
            if let Some(renderer) = &self.renderer {
                self.vertex_buffer = Some(renderer.device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("Vertex Buffer"),
                        contents: bytemuck::cast_slice(vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }
                ));
                self.index_buffer = Some(renderer.device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("Index Buffer"),
                        contents: bytemuck::cast_slice(indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }
                ));
                self.num_indices = indices.len() as u32;
            }
            
            let elapsed = start.elapsed().as_millis();
            println!("[Upload] Complete in {}ms", elapsed);
            
            *status = MeshStatus::Idle;
        }
    }
}

fn generate_mesh(world: &RwLock<ContinuousWorld>, cam_pos: [f64; 3]) -> (Vec<Vertex>, Vec<u32>) {
    let mut world_write = world.write().unwrap();
    let blocks = world_write.query_lod(cam_pos, 50.0);
    drop(world_write); // Release lock ASAP
    
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    for (block, _lod) in &blocks {
        if block.voxels.iter().all(|&v| v == AIR) { continue; }
        
        let offset = [
            block.ecef_min[0] - cam_pos[0],
            block.ecef_min[1] - cam_pos[1],
            block.ecef_min[2] - cam_pos[2],
        ];
        
        let (verts, inds) = greedy_mesh_block(&block.voxels, offset);
        
        let base_idx = vertices.len() as u32;
        vertices.extend(verts);
        indices.extend(inds.iter().map(|i| i + base_idx));
    }
    
    (vertices, indices)
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() { return; }
        
        let window_attrs = WindowAttributes::default()
            .with_title("Async Viewer - SMOOTH 60fps")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        
        let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
        
        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)));
        let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
        let skybox = SkyboxPipeline::new(&renderer.device, renderer.config.format);
        
        let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
        let center_ecef = gps_to_ecef(&gps_center);
        let center = [center_ecef.x, center_ecef.y, center_ecef.z];
        
        let world = Arc::new(RwLock::new(ContinuousWorld::new(center, 100.0).unwrap()));
        
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.pipeline = Some(pipeline);
        self.skybox = Some(skybox);
        self.continuous_world = Some(world);
        
        println!("\n✓ Ready! Requesting initial mesh async...\n");
        
        let cam_pos = [self.camera.position.x, self.camera.position.y, self.camera.position.z];
        self.request_mesh_update(cam_pos);
    }
    
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if event.state.is_pressed() {
                        self.keys_pressed.insert(code);
                        if code == KeyCode::Escape {
                            event_loop.exit();
                        }
                    } else {
                        self.keys_pressed.remove(&code);
                    }
                }
            }
            
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                if let Some(window) = &self.window {
                    self.mouse_captured = true;
                    window.set_cursor_visible(false);
                    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                }
            }
            
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size);
                }
            }
            
            WindowEvent::RedrawRequested => {
                let now = std::time::Instant::now();
                let delta_time = now.duration_since(self.last_frame_time).as_secs_f64();
                self.last_frame_time = now;
                
                // ALWAYS handle input and move camera (NEVER skip this!)
                self.handle_input(delta_time);
                
                // Get current camera position
                let cam_pos = [self.camera.position.x, self.camera.position.y, self.camera.position.z];
                
                // Check if mesh is ready and upload (THIS MIGHT BLOCK - the problem!)
                self.check_and_upload_mesh(cam_pos);
                
                // Request new mesh if moved significantly
                let dist = (
                    (cam_pos[0] - self.last_mesh_camera_pos[0]).powi(2) +
                    (cam_pos[1] - self.last_mesh_camera_pos[1]).powi(2) +
                    (cam_pos[2] - self.last_mesh_camera_pos[2]).powi(2)
                ).sqrt();
                
                let time_since_request = now.duration_since(self.last_mesh_request).as_secs_f64();
                
                if dist > 10.0 && time_since_request >= 1.0 {
                    self.request_mesh_update(cam_pos);
                    self.last_mesh_request = now;
                    self.last_mesh_camera_pos = cam_pos;
                }
                
                // Render (always smooth, never blocks!)
                if let (Some(window), Some(renderer), Some(pipeline), Some(skybox)) = 
                    (&self.window, &mut self.renderer, &self.pipeline, &self.skybox) {
                    
                    let aspect = renderer.size.width as f32 / renderer.size.height as f32;
                    
                    // CRITICAL: Account for mesh origin vs current camera position
                    // Mesh vertices are relative to mesh_origin, but camera has moved
                    let offset_from_mesh = [
                        cam_pos[0] - self.mesh_origin[0],
                        cam_pos[1] - self.mesh_origin[1],
                        cam_pos[2] - self.mesh_origin[2],
                    ];
                    
                    // Create camera offset by the difference
                    let adjusted_camera_pos = glam::DVec3::new(
                        offset_from_mesh[0],
                        offset_from_mesh[1],
                        offset_from_mesh[2],
                    );
                    
                    // Temporarily move camera for view matrix
                    let original_pos = self.camera.position;
                    self.camera.position = adjusted_camera_pos;
                    let (view_proj, _) = self.camera.view_projection_matrix(aspect);
                    self.camera.position = original_pos; // Restore
                    
                    pipeline.update_uniforms(&renderer.queue, view_proj);
                    
                    let clear_color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
                    let vertex_buffer_ref = self.vertex_buffer.as_ref();
                    let index_buffer_ref = self.index_buffer.as_ref();
                    let num_indices = self.num_indices;
                    
                    renderer.render(clear_color, |render_pass| {
                        // Terrain only (skybox causes lifetime issues)
                        if let (Some(vb), Some(ib)) = (vertex_buffer_ref, index_buffer_ref) {
                            if num_indices > 0 {
                                render_pass.set_pipeline(&pipeline.pipeline);
                                render_pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
                                render_pass.set_vertex_buffer(0, vb.slice(..));
                                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                                render_pass.draw_indexed(0..num_indices, 0, 0..1);
                            }
                        }
                    }).ok();
                    
                    // FPS counter
                    self.frame_count += 1;
                    if now.duration_since(self.fps_update_time).as_secs_f64() >= 1.0 {
                        let fps = self.frame_count as f64 / now.duration_since(self.fps_update_time).as_secs_f64();
                        let pos = self.camera.position;
                        let gps = ecef_to_gps(&EcefPos { x: pos.x, y: pos.y, z: pos.z });
                        window.set_title(&format!(
                            "Async Viewer - {:.1} FPS | ({:.6}°, {:.6}°) {:.0}m",
                            fps, gps.lat_deg, gps.lon_deg, gps.elevation_m
                        ));
                        self.frame_count = 0;
                        self.fps_update_time = now;
                    }
                    
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
    
    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: winit::event::DeviceId, event: winit::event::DeviceEvent) {
        if !self.mouse_captured {
            return;
        }
        
        if let winit::event::DeviceEvent::MouseMotion { delta } = event {
            // Fixed: Reversed left/right (swap sign on delta.0)
            self.camera.rotate(-delta.1 * 0.004, -delta.0 * 0.004);
        }
    }
    
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut lat = TEST_LAT;
    let mut lon = TEST_LON;
    let mut alt = 30.0;
    let mut pitch = -30.0;
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--lat" => { lat = args.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(TEST_LAT); i += 2; }
            "--lon" => { lon = args.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(TEST_LON); i += 2; }
            "--alt" => { alt = args.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(30.0); i += 2; }
            "--pitch" => { pitch = args.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(-30.0); i += 2; }
            _ => i += 1,
        }
    }
    
    let gps = GpsPos { lat_deg: lat, lon_deg: lon, elevation_m: alt };
    let app = App::new(gps, pitch);
    
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut  {app}).unwrap();
}
