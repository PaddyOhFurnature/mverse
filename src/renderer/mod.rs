//! Rendering system using wgpu
//!
//! This module handles all graphics rendering for the metaverse.
//! Uses floating-origin rendering to handle large ECEF coordinates.

pub mod pipeline;
pub mod camera;
pub mod shaders;
pub mod mesh;
pub mod greedy_mesh;

use wgpu;
use winit::window::Window;
use std::sync::Arc;

/// Main renderer state
pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
}

impl Renderer {
    /// Create a new renderer for the given window
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        // Create wgpu instance
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // Create surface
        let surface = instance.create_surface(window).unwrap();

        // Request adapter
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        // Request device and queue
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    label: None,
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .unwrap();

        // Configure surface
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        
        // Create depth texture
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            surface,
            device,
            queue,
            config,
            size,
            depth_texture,
            depth_view,
        }
    }

    /// Resize the renderer
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            
            // Recreate depth texture with new size
            self.depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Depth Texture"),
                size: wgpu::Extent3d {
                    width: new_size.width,
                    height: new_size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.depth_view = self.depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        }
    }

    /// Render a frame with the given clear color and optional render callback
    pub fn render<F>(&mut self, clear_color: wgpu::Color, render_fn: F) -> Result<(), wgpu::SurfaceError>
    where
        F: FnOnce(&mut wgpu::RenderPass),
    {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            
            // Call the render function to draw geometry
            render_fn(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
    
    /// Capture the current frame as RGBA8 data for screenshot
    pub fn capture_frame(&mut self) -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
        // Create a texture to copy the surface into
        let texture_desc = wgpu::TextureDescriptor {
            label: Some("Screenshot Texture"),
            size: wgpu::Extent3d {
                width: self.size.width,
                height: self.size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        
        let screenshot_texture = self.device.create_texture(&texture_desc);
        
        // Create a buffer to copy pixels into
        let bytes_per_pixel = 4; // RGBA8
        let unpadded_bytes_per_row = self.size.width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) / align * align;
        let buffer_size = (padded_bytes_per_row * self.size.height) as u64;
        
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screenshot Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        
        // Get the current surface texture
        let output = self.surface.get_current_texture()?;
        let view = screenshot_texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Render to our screenshot texture
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Screenshot Encoder"),
        });
        
        // Copy surface to our texture (blit)
        let surface_view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // We need to re-render the frame - just clear for now
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Screenshot Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.5,
                            g: 0.7,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
        }
        
        // Copy texture to buffer
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &screenshot_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.size.height),
                },
            },
            texture_desc.size,
        );
        
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        
        // Read buffer
        let buffer_slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()??;
        
        let data = buffer_slice.get_mapped_range();
        
        // Copy and un-pad the data
        let mut pixels = vec![0u8; (unpadded_bytes_per_row * self.size.height) as usize];
        for row in 0..self.size.height {
            let src_start = (row * padded_bytes_per_row) as usize;
            let src_end = src_start + unpadded_bytes_per_row as usize;
            let dst_start = (row * unpadded_bytes_per_row) as usize;
            let dst_end = dst_start + unpadded_bytes_per_row as usize;
            pixels[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
        }
        
        drop(data);
        buffer.unmap();
        
        Ok((pixels, self.size.width, self.size.height))
    }
    
    /// Render a frame to an offscreen texture and capture it as RGBA8 data
    pub fn render_and_capture<F>(
        &mut self, 
        clear_color: wgpu::Color, 
        render_fn: F
    ) -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>>
    where
        F: FnOnce(&mut wgpu::RenderPass),
    {
        // Create a texture to render into (with COPY_SRC so we can read it back)
        let texture_desc = wgpu::TextureDescriptor {
            label: Some("Screenshot Texture"),
            size: wgpu::Extent3d {
                width: self.size.width,
                height: self.size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format, // Use same format as surface
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        
        let screenshot_texture = self.device.create_texture(&texture_desc);
        let view = screenshot_texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Create depth texture for this render
        let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Screenshot Depth Texture"),
            size: wgpu::Extent3d {
                width: self.size.width,
                height: self.size.height,
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
        
        // Create buffer to copy pixels into
        let bytes_per_pixel = 4; // RGBA8
        let unpadded_bytes_per_row = self.size.width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) / align * align;
        let buffer_size = (padded_bytes_per_row * self.size.height) as u64;
        
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screenshot Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        
        // Render to our screenshot texture
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Screenshot Encoder"),
        });
        
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Screenshot Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
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
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            
            // Call the render function to draw geometry
            render_fn(&mut render_pass);
        }
        
        // Copy texture to buffer
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &screenshot_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.size.height),
                },
            },
            texture_desc.size,
        );
        
        self.queue.submit(std::iter::once(encoder.finish()));
        
        // Read buffer back to CPU
        let buffer_slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()??;
        
        let data = buffer_slice.get_mapped_range();
        
        // Copy and un-pad the data
        let mut pixels = vec![0u8; (unpadded_bytes_per_row * self.size.height) as usize];
        for row in 0..self.size.height {
            let src_start = (row * padded_bytes_per_row) as usize;
            let src_end = src_start + unpadded_bytes_per_row as usize;
            let dst_start = (row * unpadded_bytes_per_row) as usize;
            let dst_end = dst_start + unpadded_bytes_per_row as usize;
            pixels[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
        }
        
        drop(data);
        buffer.unmap();
        
        Ok((pixels, self.size.width, self.size.height))
    }
}
