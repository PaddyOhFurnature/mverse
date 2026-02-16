//! Skybox rendering with gradient sky
//!
//! Renders a simple gradient background from horizon to zenith.

use wgpu;

/// Skybox shader (vertex + fragment)
pub const SKYBOX_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) view_dir: vec3<f32>,
}

// Fullscreen triangle vertices (covers entire screen)
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    
    // Generate fullscreen triangle
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    
    out.position = vec4<f32>(x, y, 0.999, 1.0); // At far plane
    out.view_dir = vec3<f32>(x, y, -1.0); // Direction for gradient
    
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Normalize view direction
    let dir = normalize(in.view_dir);
    
    // Gradient from horizon to zenith
    let horizon_color = vec3<f32>(0.6, 0.7, 0.9);  // Light blue
    let zenith_color = vec3<f32>(0.2, 0.4, 0.8);   // Deep blue
    
    // Vertical gradient (Y component)
    let t = clamp((dir.y + 1.0) * 0.5, 0.0, 1.0);
    let sky_color = mix(horizon_color, zenith_color, t);
    
    return vec4<f32>(sky_color, 1.0);
}
"#;

/// Skybox render pipeline
pub struct SkyboxPipeline {
    pipeline: wgpu::RenderPipeline,
}

impl SkyboxPipeline {
    /// Create a new skybox pipeline
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Skybox Shader"),
            source: wgpu::ShaderSource::Wgsl(SKYBOX_SHADER.into()),
        });
        
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Skybox Pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // No culling for fullscreen triangle
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false, // Skybox doesn't write depth
                depth_compare: wgpu::CompareFunction::LessEqual, // Draw at far plane
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        
        Self { pipeline }
    }
    
    /// Render the skybox
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.draw(0..3, 0..1); // Fullscreen triangle (3 vertices)
    }
}
