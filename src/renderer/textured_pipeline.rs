//! Textured wgpu pipeline for rendering GLB models.

#[allow(unused_imports)]
use wgpu::util::DeviceExt;

/// Vertex format for GLB model rendering: position + normal + UV.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexturedVertex {
    pub position: [f32; 3],
    pub normal:   [f32; 3],
    pub uv:       [f32; 2],
}

impl TexturedVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TexturedVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// A single loaded GLB model ready for GPU rendering.
#[allow(dead_code)]
pub struct GlbModel {
    vertex_buffer:       wgpu::Buffer,
    index_buffer:        wgpu::Buffer,
    pub index_count:     u32,
    pub texture_bind_group: wgpu::BindGroup,
    /// True if the model has a real texture or non-white base colour.
    /// False means the texture fell back to a 1×1 white pixel — model
    /// will render as solid white, which is a sign the GLB has no
    /// usable texture data.
    pub has_real_texture: bool,
}

impl GlbModel {
    pub fn draw<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.index_count, 0, 0..1);
    }
}

/// Pipeline for rendering textured GLB models with position/normal/UV vertices.
pub struct TexturedPipeline {
    pipeline: wgpu::RenderPipeline,
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
}

impl TexturedPipeline {
    pub fn new(
        context: &crate::renderer::RenderContext,
        camera_layout: &wgpu::BindGroupLayout,
        model_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let device = &context.device;

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("textured_texture_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Textured Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("textured_shader.wgsl").into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Textured Pipeline Layout"),
            bind_group_layouts: &[camera_layout, model_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Textured Render Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[TexturedVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: context.config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        Self { pipeline, texture_bind_group_layout }
    }

    /// Load a GLB file from disk and upload its first mesh/primitive to the GPU.
    pub fn load_glb(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &str,
    ) -> Option<GlbModel> {
        let (document, buffers, _images) = gltf::import(path).ok()?;

        let mesh = document.meshes().next()?;
        let primitive = mesh.primitives().next()?;

        let reader = primitive.reader(|buf| Some(&buffers[buf.index()]));

        let positions: Vec<[f32; 3]> = reader.read_positions()?.collect();
        let normals: Vec<[f32; 3]> = reader
            .read_normals()
            .map(|n| n.collect())
            .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
        let uvs: Vec<[f32; 2]> = reader
            .read_tex_coords(0)
            .map(|tc| tc.into_f32().collect())
            .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);

        let vertices: Vec<TexturedVertex> = positions
            .iter()
            .zip(normals.iter())
            .zip(uvs.iter())
            .map(|((pos, nor), uv)| TexturedVertex {
                position: *pos,
                normal: *nor,
                uv: *uv,
            })
            .collect();

        let indices: Vec<u32> = reader
            .read_indices()
            .map(|idx| idx.into_u32().collect())
            .unwrap_or_else(|| (0..vertices.len() as u32).collect());

        if vertices.is_empty() || indices.is_empty() {
            return None;
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("GLB Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("GLB Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Load the colormap texture by resolving the external URI ourselves.
        // gltf::import may silently return a 1x1 white image when it can't find
        // the external file, which renders everything white.  We explicitly read
        // the file from disk relative to the GLB's directory so we always get
        // the real pixels — or fall back to solid base_color_factor if missing.
        let pbr = primitive.material().pbr_metallic_roughness();
        let glb_dir = std::path::Path::new(path).parent()
            .unwrap_or(std::path::Path::new("."));

        let (tex_data, tex_w, tex_h, has_real_texture) =
            if let Some(tex_info) = pbr.base_color_texture() {
                let src = tex_info.texture().source().source();
                let explicit_path = match &src {
                    gltf::image::Source::Uri { uri, .. } => Some(glb_dir.join(uri)),
                    _ => None,
                };
                let loaded = explicit_path
                    .and_then(|p| std::fs::read(p).ok())
                    .and_then(|bytes| image::load_from_memory(&bytes).ok())
                    .map(|img| img.to_rgba8());
                if let Some(rgba_img) = loaded {
                    let w = rgba_img.width();
                    let h = rgba_img.height();
                    (rgba_img.into_raw(), w, h, true)
                } else {
                    let f = pbr.base_color_factor();
                    let pixel = [(f[0]*255.0) as u8, (f[1]*255.0) as u8,
                                 (f[2]*255.0) as u8, (f[3]*255.0) as u8];
                    let is_white = f[0] > 0.99 && f[1] > 0.99 && f[2] > 0.99;
                    (pixel.to_vec(), 1u32, 1u32, !is_white)
                }
            } else {
                let f = pbr.base_color_factor();
                let pixel = [(f[0]*255.0) as u8, (f[1]*255.0) as u8,
                             (f[2]*255.0) as u8, (f[3]*255.0) as u8];
                let is_white = f[0] > 0.99 && f[1] > 0.99 && f[2] > 0.99;
                (pixel.to_vec(), 1u32, 1u32, !is_white)
            };

        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("GLB Diffuse Texture"),
                size: wgpu::Extent3d {
                    width: tex_w,
                    height: tex_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &tex_data,
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("GLB Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GLB Texture BindGroup"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Some(GlbModel {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            texture_bind_group,
            has_real_texture,
        })
    }

    /// Bind this pipeline and camera bind group (group 0) on the render pass.
    pub fn set_pipeline<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        camera_bind_group: &'a wgpu::BindGroup,
    ) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, camera_bind_group, &[]);
    }

    /// Set model transform (group 1) + texture (group 2), then draw.
    pub fn draw_model<'a>(
        render_pass: &mut wgpu::RenderPass<'a>,
        model: &'a GlbModel,
        model_bind_group: &'a wgpu::BindGroup,
    ) {
        render_pass.set_bind_group(1, model_bind_group, &[]);
        render_pass.set_bind_group(2, &model.texture_bind_group, &[]);
        model.draw(render_pass);
    }
}

/// Convert any gltf image format to a tightly-packed RGBA8 buffer.
/// Returns a white 1×1 pixel (4 bytes) if conversion yields an unexpected size.
fn image_to_rgba8(img: &gltf::image::Data) -> Vec<u8> {
    let expected = (img.width * img.height * 4) as usize;
    let result: Vec<u8> = match img.format {
        gltf::image::Format::R8G8B8A8 => img.pixels.clone(),
        gltf::image::Format::R8G8B8 => img
            .pixels
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255u8])
            .collect(),
        gltf::image::Format::R8 => img
            .pixels
            .iter()
            .flat_map(|&r| [r, r, r, 255u8])
            .collect(),
        gltf::image::Format::R8G8 => img
            .pixels
            .chunks(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        gltf::image::Format::R16 => img
            .pixels
            .chunks(2)
            .flat_map(|c| { let v = c[1]; [v, v, v, 255u8] })
            .collect(),
        gltf::image::Format::R16G16 => img
            .pixels
            .chunks(4)
            .flat_map(|c| [c[1], c[1], c[1], c[3]])
            .collect(),
        gltf::image::Format::R16G16B16 => img
            .pixels
            .chunks(6)
            .flat_map(|c| [c[1], c[3], c[5], 255u8])
            .collect(),
        gltf::image::Format::R16G16B16A16 => img
            .pixels
            .chunks(8)
            .flat_map(|c| [c[1], c[3], c[5], c[7]])
            .collect(),
        gltf::image::Format::R32G32B32FLOAT => img
            .pixels
            .chunks(12)
            .flat_map(|c| {
                let r = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                let g = f32::from_le_bytes([c[4], c[5], c[6], c[7]]);
                let b = f32::from_le_bytes([c[8], c[9], c[10], c[11]]);
                [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255u8]
            })
            .collect(),
        gltf::image::Format::R32G32B32A32FLOAT => img
            .pixels
            .chunks(16)
            .flat_map(|c| {
                let r = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                let g = f32::from_le_bytes([c[4], c[5], c[6], c[7]]);
                let b = f32::from_le_bytes([c[8], c[9], c[10], c[11]]);
                let a = f32::from_le_bytes([c[12], c[13], c[14], c[15]]);
                [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, (a * 255.0) as u8]
            })
            .collect(),
    };
    if result.len() == expected {
        result
    } else {
        vec![255u8; 4] // 1×1 white fallback
    }
}
