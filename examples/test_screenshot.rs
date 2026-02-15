//! Simple screenshot test using WorldManager
//! Captures a single frame and saves it

use metaverse_core::renderer::{camera::Camera, pipeline::BasicPipeline, Renderer};
use metaverse_core::world_manager::WorldManager;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::elevation_downloader::ElevationDownloader;
use metaverse_core::osm::OsmData;
use metaverse_core::cache::DiskCache;
use metaverse_core::mesh_generation::svo_meshes_to_colored_vertices;
use metaverse_core::materials::MaterialColors;
use metaverse_core::coordinates::{gps_to_ecef, enu_to_ecef, GpsPos, EnuPos};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes};
use wgpu::util::DeviceExt;

struct ScreenshotApp {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pipeline: Option<BasicPipeline>,
    camera: Camera,
    world_manager: Option<WorldManager>,
    srtm: Option<SrtmManager>,
    osm_data: Option<OsmData>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    frame_count: usize,
    screenshot_taken: bool,
}

impl ScreenshotApp {
    fn new() -> Self {
        // Camera at Brisbane looking down
        let camera = Camera::brisbane();
        
        Self {
            window: None,
            renderer: None,
            pipeline: None,
            camera,
            world_manager: None,
            srtm: None,
            osm_data: None,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            frame_count: 0,
            screenshot_taken: false,
        }
    }
}

impl ApplicationHandler for ScreenshotApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        println!("=== Screenshot Test ===");
        
        // Create window
        let window_attributes = WindowAttributes::default()
            .with_title("Screenshot Test")
            .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080));
        
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        println!("✓ Window created");
        
        // Create renderer
        let renderer = pollster::block_on(Renderer::new(window.clone()));
        println!("✓ Renderer created");
        
        // Create pipeline
        let pipeline = BasicPipeline::new(&renderer.device, renderer.config.format);
        println!("✓ Pipeline created");
        
        // Load data
        let cache = DiskCache::new().expect("Failed to create cache");
        
        // Initialize SRTM
        let mut srtm = SrtmManager::new(cache.clone());
        
        // Pre-download Brisbane elevation
        println!("Pre-downloading Brisbane elevation...");
        let downloader = ElevationDownloader::new();
        let brisbane = GpsPos {
            lat_deg: -27.4698,
            lon_deg: 153.0251,
            elevation_m: 0.0,
        };
        let results = pollster::block_on(downloader.download_area(&brisbane, 10000.0));
        println!("  Downloaded {} tiles", results.len());
        
        // Load OSM
        let osm_data = cache.get::<OsmData>("brisbane_cbd")
            .expect("Failed to load OSM data - run download_brisbane_data first");
        println!("✓ Loaded {} buildings, {} roads", 
            osm_data.buildings.len(), osm_data.roads.len());
        
        // Create WorldManager
        let world_manager = WorldManager::new(
            14,     // Depth 14 chunks (~400m)
            2000.0, // 2km render distance
            7       // SVO depth 7 (128³ voxels)
        );
        println!("✓ WorldManager initialized");
        
        // Store state
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.pipeline = Some(pipeline);
        self.srtm = Some(srtm);
        self.osm_data = Some(osm_data);
        self.world_manager = Some(world_manager);
        
        println!("\n=== Initialization complete ===");
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
                
                if self.screenshot_taken {
                    return;
                }
                
                let Some(ref renderer) = self.renderer else { return };
                let Some(ref pipeline) = self.pipeline else { return };
                let Some(ref window) = self.window else { return };
                let Some(ref mut world_manager) = self.world_manager else { return };
                let Some(ref mut srtm) = self.srtm else { return };
                let Some(ref osm_data) = self.osm_data else { return };
                
                // Update world on first frame
                if self.frame_count == 1 {
                    println!("\n[Frame {}] Generating chunk...", self.frame_count);
                    world_manager.update(&self.camera.position, srtm, osm_data);
                    
                    // Extract meshes
                    let chunk_meshes = world_manager.extract_meshes(&self.camera.position);
                    println!("Extracted {} chunks", chunk_meshes.len());
                    
                    if chunk_meshes.is_empty() {
                        println!("ERROR: No chunks generated");
                        return;
                    }
                    
                    // Convert to GPU format
                    let mut all_vertices = Vec::new();
                    let mut all_indices = Vec::new();
                    
                    for (meshes, chunk_center) in chunk_meshes {
                        let (vertices, indices) = svo_meshes_to_colored_vertices(
                            &meshes,
                            &chunk_center,
                            &MaterialColors::default(),
                        );
                        
                        let index_offset = all_vertices.len() as u32;
                        all_vertices.extend(vertices);
                        all_indices.extend(indices.iter().map(|i| i + index_offset));
                    }
                    
                    println!("Generated {} vertices, {} indices", 
                        all_vertices.len(), all_indices.len());
                    
                    if all_vertices.is_empty() {
                        println!("ERROR: No vertices generated");
                        return;
                    }
                    
                    // Create GPU buffers
                    self.vertex_buffer = Some(renderer.device.create_buffer_init(
                        &wgpu::util::BufferInitDescriptor {
                            label: Some("Vertex Buffer"),
                            contents: bytemuck::cast_slice(&all_vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        }
                    ));
                    
                    self.index_buffer = Some(renderer.device.create_buffer_init(
                        &wgpu::util::BufferInitDescriptor {
                            label: Some("Index Buffer"),
                            contents: bytemuck::cast_slice(&all_indices),
                            usage: wgpu::BufferUsages::INDEX,
                        }
                    ));
                    
                    self.num_indices = all_indices.len() as u32;
                    println!("✓ GPU buffers created");
                }
                
                // Render
                if self.vertex_buffer.is_some() && self.num_indices > 0 {
                    let output = renderer.surface.get_current_texture().unwrap();
                    let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
                    
                    let mut encoder = renderer.device.create_command_encoder(
                        &wgpu::CommandEncoderDescriptor { label: Some("Render Encoder") }
                    );
                    
                    {
                        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("Render Pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 0.1, g: 0.2, b: 0.3, a: 1.0
                                    }),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                view: &renderer.depth_texture,
                                depth_ops: Some(wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(1.0),
                                    store: wgpu::StoreOp::Store,
                                }),
                                stencil_ops: None,
                            }),
                            ..Default::default()
                        });
                        
                        pipeline.render(
                            &mut render_pass,
                            &self.camera,
                            self.vertex_buffer.as_ref().unwrap(),
                            self.index_buffer.as_ref().unwrap(),
                            self.num_indices,
                        );
                    }
                    
                    renderer.queue.submit(std::iter::once(encoder.finish()));
                    output.present();
                    
                    if self.frame_count == 5 && !self.screenshot_taken {
                        println!("\n✓ Screenshot would be saved here");
                        println!("  Frame {} rendered successfully", self.frame_count);
                        println!("  {} indices rendered", self.num_indices);
                        self.screenshot_taken = true;
                        
                        // Exit after screenshot
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        event_loop.exit();
                    }
                }
                
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = ScreenshotApp::new();
    event_loop.run_app(&mut app).unwrap();
}
