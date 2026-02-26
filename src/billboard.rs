//! Billboard system for Construct module room walls.
//!
//! Each module room's far wall (the "screen wall") shows Meshsite content
//! as a grid of textured quads (billboards). Content is rendered into an
//! RGBA texture using a classic 8×8 pixel font — terminal aesthetic,
//! fitting the BBS room concept.
//!
//! Pipeline:
//!   `ContentItem` → `render_item_to_rgba()` → wgpu Texture → `GpuBillboard`
//!   `RoomTemplate::billboard_transforms()` → world-space positions
//!   `BillboardPipeline::render()` → textured quads in the render pass
//!
//! The billboard pipeline runs inside the SAME render pass as the main scene
//! pipeline so depth testing between walls and billboard quads works correctly.

use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::meshsite::{ContentItem, Section};
use crate::renderer::RenderContext;

// ── Texture resolution ────────────────────────────────────────────────────────

/// Billboard texture size (pixels).
const TEX_W: u32 = 512;
const TEX_H: u32 = 320;

// ── Physical dimensions ───────────────────────────────────────────────────────

/// Billboard width in metres — fits inside the 8 m wide room with margins.
pub const BILLBOARD_W: f32 = 5.0;
/// Billboard height in metres — fits inside the 3 m tall room.
pub const BILLBOARD_H: f32 = 2.4;
/// Gap between adjacent billboards (metres).
const BB_GAP_H: f32 = 0.28;
const BB_GAP_V: f32 = 0.22;
/// Depth of each module room (metres) — used to position the screen wall.
const ROOM_DEPTH: f32 = 6.0;
/// Height of billboard centre above the floor.
const EYE_LEVEL_Y: f32 = 1.2;

// ── Room template ─────────────────────────────────────────────────────────────

/// Describes the layout and style of billboards in one module room.
#[derive(Clone)]
pub struct RoomTemplate {
    /// Number of columns of billboards on the primary (back) wall.
    pub cols: usize,
    /// Number of rows.
    pub rows: usize,
    /// Background colour (RGBA).
    pub bg:     [u8; 4],
    /// Primary text colour.
    pub text:   [u8; 4],
    /// Metadata text colour (author, date, etc.).
    pub meta:   [u8; 4],
    /// Accent / title colour.
    pub accent: [u8; 4],
}

impl RoomTemplate {
    /// Default template for each content section.
    pub fn for_section(section: &Section) -> Self {
        match section {
            Section::Forums => Self {
                cols: 1, rows: 1,
                bg:     [10, 14, 20, 255],
                text:   [210, 220, 235, 255],
                meta:   [90, 105, 130, 255],
                accent: [88, 166, 255, 255],
            },
            Section::Wiki => Self {
                cols: 1, rows: 1,
                bg:     [10, 18, 12, 255],
                text:   [205, 235, 210, 255],
                meta:   [80, 115, 85, 255],
                accent: [72, 200, 100, 255],
            },
            Section::Marketplace => Self {
                cols: 1, rows: 1,
                bg:     [20, 12, 26, 255],
                text:   [230, 218, 242, 255],
                meta:   [110, 88, 130, 255],
                accent: [195, 120, 255, 255],
            },
            Section::Post => Self {
                cols: 1, rows: 1,
                bg:     [20, 16, 8, 255],
                text:   [238, 230, 210, 255],
                meta:   [120, 108, 78, 255],
                accent: [255, 195, 70, 255],
            },
        }
    }

    /// Number of billboard slots in this template.
    pub fn slot_count(&self) -> usize {
        self.cols * self.rows
    }

    /// World-space transforms for each billboard slot (row-major).
    ///
    /// `room_center` is the centre of the module room in world space.
    /// `outward_normal` points from the plaza toward the room (outward from plaza).
    ///
    /// Returns `(centre, normal, right, up)` per slot.
    pub fn billboard_transforms(
        &self,
        room_center: Vec3,
        outward_normal: Vec3,
    ) -> Vec<(Vec3, Vec3, Vec3, Vec3)> {
        // Screen wall is the FAR wall — deep in the room, away from the door.
        // room_center is midway along the room. Move further in the outward direction
        // to reach the back wall, then pull back 0.12 m to sit in front of it.
        let inward = -outward_normal;
        let wall_c = room_center + outward_normal * (ROOM_DEPTH * 0.5 - 0.12);

        // right: the axis that is "to the right" when standing inside facing the back wall.
        // When facing `outward_normal`, right = forward × world_up.
        let world_up = Vec3::Y;
        let right = outward_normal.cross(world_up);
        let right = if right.length_squared() > 0.001 { right.normalize() } else { Vec3::X };
        let up = world_up;
        // Billboard normal points inward so the player walking in sees the front face.
        let billboard_normal = inward;

        // Compute total grid dimensions
        let total_w = self.cols as f32 * BILLBOARD_W + (self.cols.saturating_sub(1)) as f32 * BB_GAP_H;
        let total_h = self.rows as f32 * BILLBOARD_H + (self.rows.saturating_sub(1)) as f32 * BB_GAP_V;

        // Top-left slot position (relative to wall centre)
        let start_x = -(total_w * 0.5) + BILLBOARD_W * 0.5;
        let start_y =  (total_h * 0.5) - BILLBOARD_H * 0.5 + EYE_LEVEL_Y;

        let mut out = Vec::new();
        for row in 0..self.rows {
            for col in 0..self.cols {
                let ox = start_x + col as f32 * (BILLBOARD_W + BB_GAP_H);
                let oy = start_y - row as f32 * (BILLBOARD_H + BB_GAP_V);
                let centre = wall_c + right * ox + up * oy;
                out.push((centre, billboard_normal, right, up));
            }
        }
        out
    }
}

// ── Vertex format ─────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BillboardVertex {
    position: [f32; 3],
    normal:   [f32; 3],
    uv:       [f32; 2],
}

impl BillboardVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

// ── GPU billboard ─────────────────────────────────────────────────────────────

/// A single GPU-resident textured billboard quad.
pub struct GpuBillboard {
    vertex_buf:           wgpu::Buffer,
    index_buf:            wgpu::Buffer,
    model_bind_group:     wgpu::BindGroup,
    texture_bind_group:   wgpu::BindGroup,
    texture:              wgpu::Texture,
    tex_w:                u32,
    tex_h:                u32,
}

impl GpuBillboard {
    fn new(
        device:      &wgpu::Device,
        queue:       &wgpu::Queue,
        center:      Vec3,
        normal:      Vec3,
        right:       Vec3,
        up:          Vec3,
        rgba_pixels: &[u8],
        model_bgl:   &wgpu::BindGroupLayout,
        tex_bgl:     &wgpu::BindGroupLayout,
        sampler:     &wgpu::Sampler,
    ) -> Self {
        Self::new_sized(device, queue, center, normal, right, up,
            rgba_pixels, TEX_W, TEX_H, BILLBOARD_W, BILLBOARD_H,
            model_bgl, tex_bgl, sampler)
    }

    fn new_sized(
        device:      &wgpu::Device,
        queue:       &wgpu::Queue,
        center:      Vec3,
        normal:      Vec3,
        right:       Vec3,
        up:          Vec3,
        rgba_pixels: &[u8],
        tex_w:       u32,
        tex_h:       u32,
        phys_w:      f32,
        phys_h:      f32,
        model_bgl:   &wgpu::BindGroupLayout,
        tex_bgl:     &wgpu::BindGroupLayout,
        sampler:     &wgpu::Sampler,
    ) -> Self {
        let hw = phys_w * 0.5;
        let hh = phys_h * 0.5;
        let n  = normal.to_array();

        let verts = [
            BillboardVertex { position: (center - right*hw - up*hh).to_array(), normal: n, uv: [0.0, 1.0] },
            BillboardVertex { position: (center + right*hw - up*hh).to_array(), normal: n, uv: [1.0, 1.0] },
            BillboardVertex { position: (center + right*hw + up*hh).to_array(), normal: n, uv: [1.0, 0.0] },
            BillboardVertex { position: (center - right*hw + up*hh).to_array(), normal: n, uv: [0.0, 0.0] },
        ];
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];

        let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("BB VB"), contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("BB IB"), contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Model buffer (identity — position is baked into vertices)
        let model_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("BB Model"),
            contents: bytemuck::cast_slice(Mat4::IDENTITY.as_ref()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: model_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: model_buf.as_entire_binding() }],
            label: Some("BB Model BG"),
        });

        // Texture
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("BB Tex"),
            size: wgpu::Extent3d { width: tex_w, height: tex_h, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture, mip_level: 0,
                origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All,
            },
            rgba_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(tex_w * 4),
                rows_per_image: Some(tex_h),
            },
            wgpu::Extent3d { width: tex_w, height: tex_h, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: tex_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
            label: Some("BB Tex BG"),
        });

        Self { vertex_buf, index_buf, model_bind_group, texture_bind_group, texture, tex_w, tex_h }
    }

    /// Upload new RGBA pixels to the existing texture (same dimensions).
    pub fn update_texture(&self, queue: &wgpu::Queue, rgba_pixels: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture, mip_level: 0,
                origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All,
            },
            rgba_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.tex_w * 4),
                rows_per_image: Some(self.tex_h),
            },
            wgpu::Extent3d { width: self.tex_w, height: self.tex_h, depth_or_array_layers: 1 },
        );
    }

    /// Draw this billboard. Call after `BillboardPipeline::begin_render()`.
    pub fn render<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>) {
        rpass.set_bind_group(1, &self.model_bind_group, &[]);
        rpass.set_bind_group(2, &self.texture_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buf.slice(..));
        rpass.set_index_buffer(self.index_buf.slice(..), wgpu::IndexFormat::Uint16);
        rpass.draw_indexed(0..6, 0, 0..1);
    }
}

// ── Module billboard set ───────────────────────────────────────────────────────

/// All billboards for one Construct module room (one content section).
pub struct ModuleBillboards {
    pub section: Section,
    billboards: Vec<GpuBillboard>,
    /// Content IDs currently displayed — used to detect when content changes.
    pub content_ids: Vec<String>,
}

impl ModuleBillboards {
    /// Build (or rebuild) all billboard quads for one module room.
    ///
    /// `items` is ordered newest-first. Slots with no item show a placeholder.
    pub fn build(
        device:         &wgpu::Device,
        queue:          &wgpu::Queue,
        pipeline:       &BillboardPipeline,
        section:        Section,
        items:          &[ContentItem],
        room_center:    Vec3,
        outward_normal: Vec3,
    ) -> Self {
        let template   = RoomTemplate::for_section(&section);
        let transforms = template.billboard_transforms(room_center, outward_normal);

        let mut billboards  = Vec::with_capacity(transforms.len());
        let mut content_ids = Vec::with_capacity(transforms.len());

        for (slot, (center, normal, right, up)) in transforms.iter().enumerate() {
            let rgba = if slot < items.len() {
                render_item_to_rgba(&items[slot], &template)
            } else {
                render_empty_slot(&template)
            };
            let id = items.get(slot).map(|i| i.id.clone()).unwrap_or_default();
            billboards.push(GpuBillboard::new(
                device, queue,
                *center, *normal, *right, *up,
                &rgba,
                &pipeline.model_bgl,
                &pipeline.texture_bgl,
                &pipeline.sampler,
            ));
            content_ids.push(id);
        }

        Self { section, billboards, content_ids }
    }

    /// Render all billboards. Call between `begin_render()` and end of render pass.
    pub fn render<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>) {
        for bb in &self.billboards {
            bb.render(rpass);
        }
    }

    /// Returns true if the content IDs have changed vs the current item list.
    pub fn needs_rebuild(&self, items: &[ContentItem]) -> bool {
        if items.len() != self.content_ids.iter().filter(|id| !id.is_empty()).count() {
            return true;
        }
        for (i, item) in items.iter().enumerate() {
            if self.content_ids.get(i).map(|s| s.as_str()) != Some(&item.id) {
                return true;
            }
        }
        false
    }
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Render pipeline for textured billboard quads.
///
/// Uses three bind group slots:
///   Group 0: camera (shared layout with main pipeline — same descriptor)
///   Group 1: per-billboard model matrix (identity — position baked into verts)
///   Group 2: texture + sampler
pub struct BillboardPipeline {
    pipeline:    wgpu::RenderPipeline,
    cam_buf:     wgpu::Buffer,
    cam_bg:      wgpu::BindGroup,
    model_bgl:   wgpu::BindGroupLayout,
    texture_bgl: wgpu::BindGroupLayout,
    sampler:     wgpu::Sampler,
}

impl BillboardPipeline {
    pub fn new(context: &RenderContext) -> Self {
        let device = &context.device;

        // ── Group 0: Camera uniform ───────────────────────────────────────────
        // Same layout descriptor as the main pipeline's group 0.
        let cam_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("BB Cam BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let cam_init = [[0f32; 4]; 4]; // identity; will be written each frame
        let cam_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("BB Cam Buf"),
            contents: bytemuck::cast_slice(&cam_init),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let cam_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &cam_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: cam_buf.as_entire_binding() }],
            label: Some("BB Cam BG"),
        });

        // ── Group 1: Model matrix ─────────────────────────────────────────────
        let model_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("BB Model BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // ── Group 2: Texture + sampler ────────────────────────────────────────
        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("BB Tex BGL"),
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

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("BB Pipeline Layout"),
            bind_group_layouts: &[&cam_bgl, &model_bgl, &texture_bgl],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Billboard Shader"),
            source: wgpu::ShaderSource::Wgsl(BILLBOARD_WGSL.into()),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Billboard Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[BillboardVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: context.config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back), // only visible from inside the room
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: -1, // slight bias so billboard sits just in front of the wall
                    ..Default::default()
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("BB Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest, // crisp pixel font
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self { pipeline, cam_buf, cam_bg, model_bgl, texture_bgl, sampler }
    }

    /// Write the current camera view-projection matrix to the billboard camera buffer.
    /// Call once per frame before rendering billboards.
    pub fn update_camera(&self, queue: &wgpu::Queue, view_proj: &glam::Mat4) {
        let data: [[f32; 4]; 4] = view_proj.to_cols_array_2d();
        queue.write_buffer(&self.cam_buf, 0, bytemuck::cast_slice(&data));
    }

    /// Begin billboard rendering within an existing render pass.
    /// Sets the pipeline and the camera bind group (group 0).
    pub fn begin_render<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>) {
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.cam_bg, &[]);
    }
}

// ── WGSL Shader ───────────────────────────────────────────────────────────────

const BILLBOARD_WGSL: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;

struct Model {
    model: mat4x4<f32>,
};
@group(1) @binding(0) var<uniform> model: Model;

@group(2) @binding(0) var t_diffuse: texture_2d<f32>;
@group(2) @binding(1) var s_diffuse: sampler;

struct VIn {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
};
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    var out: VOut;
    // Position is already in world space — model matrix is identity.
    out.clip_pos = camera.view_proj * model.model * vec4<f32>(in.position, 1.0);
    // Flip U so the texture reads left-to-right when standing inside the room.
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return textureSample(t_diffuse, s_diffuse, in.uv);
}
"#;

// ── Terminal screen surface ───────────────────────────────────────────────────

/// A WORLDNET terminal screen — a single updatable billboard quad sized to
/// fit the top face of the kiosk terminal mesh (1.0m wide × 0.7m deep,
/// normal pointing up, rendered into a WORLDNET_W × WORLDNET_H texture).
pub struct TerminalScreen {
    quad: GpuBillboard,
}

impl TerminalScreen {
    /// Build the terminal screen at the given kiosk position.
    pub fn new(
        context:   &RenderContext,
        pipeline:  &BillboardPipeline,
        kiosk_pos: Vec3,
    ) -> Self {
        use crate::worldnet::{WORLDNET_W, WORLDNET_H};
        // Top face sits at kiosk_pos.y + 1.1 (post) + 0.025 (half of 0.05 screen thickness)
        let center = Vec3::new(kiosk_pos.x, kiosk_pos.y + 1.125, kiosk_pos.z);
        let normal = Vec3::Y;        // facing up
        let right  = Vec3::X;        // texture right = +X
        let up     = Vec3::NEG_Z;    // texture "up" = toward -Z (away from player spawn)

        let blank = vec![12u8, 14, 20, 255].repeat((WORLDNET_W * WORLDNET_H) as usize);

        let quad = GpuBillboard::new_sized(
            &context.device, &context.queue,
            center, normal, right, up,
            &blank,
            WORLDNET_W, WORLDNET_H,
            1.0, 0.7,
            &pipeline.model_bgl, &pipeline.texture_bgl, &pipeline.sampler,
        );
        Self { quad }
    }

    /// Upload a rendered WORLDNET pixel buffer to the screen texture.
    pub fn update(&self, queue: &wgpu::Queue, buf: &crate::worldnet::WorldnetPixelBuffer) {
        self.quad.update_texture(queue, &buf.pixels);
    }

    /// Render the screen quad inside a billboard render pass.
    pub fn render<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>) {
        self.quad.render(rpass);
    }
}

// ── Text rendering ────────────────────────────────────────────────────────────

/// Render a `ContentItem` into an RGBA bitmap (TEX_W × TEX_H).
pub fn render_item_to_rgba(item: &ContentItem, tmpl: &RoomTemplate) -> Vec<u8> {
    let w = TEX_W as usize;
    let h = TEX_H as usize;
    let mut px = vec![0u8; w * h * 4];

    // Background fill
    fill_rect(&mut px, w, 0, 0, w, h, tmpl.bg);

    // Top accent bar (5px)
    fill_rect(&mut px, w, 0, 0, w, 5, tmpl.accent);

    // Title (scale 3 → 24px tall glyphs)
    let ts = 3usize;
    draw_text_wrapped(&mut px, w, h, &item.title, 8, 10, tmpl.accent, ts, (w - 16) / (8 * ts + ts));

    // Author line (scale 2 → 16px tall)
    let ms = 2usize;
    let author_str = format!("by {}", truncate_str(&item.author, 26));
    draw_text(&mut px, w, h, &author_str, 8, 10 + 8 * ts + ts + 6, tmpl.meta, ms);

    // Separator
    let sep_y = 10 + 8 * ts + ts + 6 + 8 * ms + 5;
    fill_rect_alpha(&mut px, w, 8, sep_y, w - 16, 1, tmpl.accent, 70);

    // Body preview (up to 4 lines, scale 2)
    let body_y = sep_y + 6;
    let chars_w = (w - 16) / (8 * ms + ms / 2);
    draw_text_wrapped(&mut px, w, h, &item.body, 8, body_y, tmpl.text, ms, chars_w);

    px
}

/// Render a placeholder for an empty billboard slot.
pub fn render_empty_slot(tmpl: &RoomTemplate) -> Vec<u8> {
    let w = TEX_W as usize;
    let h = TEX_H as usize;
    let mut px = vec![0u8; w * h * 4];

    fill_rect(&mut px, w, 0, 0, w, h, tmpl.bg);

    // Dim border
    for x in 2..w-2 {
        fill_rect_alpha(&mut px, w, x, 2, 1, 1, tmpl.accent, 35);
        fill_rect_alpha(&mut px, w, x, h-3, 1, 1, tmpl.accent, 35);
    }
    for y in 2..h-2 {
        fill_rect_alpha(&mut px, w, 2, y, 1, 1, tmpl.accent, 35);
        fill_rect_alpha(&mut px, w, w-3, y, 1, 1, tmpl.accent, 35);
    }

    let msg = "[ empty ]";
    let scale = 2usize;
    let cx = (w / 2).saturating_sub(msg.len() * (8 * scale + scale) / 2);
    let cy = h / 2 - 8;
    draw_text(&mut px, w, h, msg, cx, cy, tmpl.meta, scale);
    px
}

// ── Pixel helpers ─────────────────────────────────────────────────────────────

fn set_pixel(px: &mut [u8], w: usize, x: usize, y: usize, rgba: [u8; 4]) {
    let i = (y * w + x) * 4;
    if i + 3 < px.len() {
        px[i..i+4].copy_from_slice(&rgba);
    }
}

fn fill_rect(px: &mut [u8], w: usize, x0: usize, y0: usize, rw: usize, rh: usize, rgba: [u8; 4]) {
    for y in y0..y0.saturating_add(rh) {
        for x in x0..x0.saturating_add(rw) {
            set_pixel(px, w, x, y, rgba);
        }
    }
}

fn fill_rect_alpha(px: &mut [u8], w: usize, x0: usize, y0: usize, rw: usize, rh: usize, rgb: [u8; 4], alpha: u8) {
    let mut c = rgb;
    c[3] = alpha;
    fill_rect(px, w, x0, y0, rw, rh, c);
}

// ── Text rendering ────────────────────────────────────────────────────────────

fn draw_text(px: &mut [u8], w: usize, h: usize, text: &str, ox: usize, oy: usize, color: [u8; 4], scale: usize) {
    let advance = 8 * scale + (scale / 2).max(1);
    let mut cx = ox;
    for ch in text.chars() {
        if cx + 8 * scale > w { break; }
        draw_glyph(px, w, h, ch, cx, oy, color, scale);
        cx += advance;
    }
}

fn draw_text_wrapped(
    px: &mut [u8], w: usize, h: usize,
    text: &str, ox: usize, oy: usize,
    color: [u8; 4], scale: usize,
    chars_per_line: usize,
) {
    let line_h = 8 * scale + scale + 2;
    let mut line_y = oy;
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut line = String::new();
    let max_lines = 4;
    let mut lines_drawn = 0;

    for word in &words {
        if line.len() + word.len() + 1 > chars_per_line && !line.is_empty() {
            draw_text(px, w, h, &line, ox, line_y, color, scale);
            line.clear();
            line_y += line_h;
            lines_drawn += 1;
            if lines_drawn >= max_lines || line_y + 8 * scale > h { return; }
        }
        if !line.is_empty() { line.push(' '); }
        line.push_str(word);
    }
    if !line.is_empty() && line_y + 8 * scale <= h {
        draw_text(px, w, h, &line, ox, line_y, color, scale);
    }
}

fn draw_glyph(px: &mut [u8], w: usize, h: usize, ch: char, ox: usize, oy: usize, color: [u8; 4], scale: usize) {
    let glyph = get_glyph(ch);
    for row in 0..8usize {
        let byte = glyph[row];
        for col in 0..8usize {
            if byte & (0x01 << col) != 0 {
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px_ = ox + col * scale + sx;
                        let py_ = oy + row * scale + sy;
                        if px_ < w && py_ < h {
                            set_pixel(px, w, px_, py_, color);
                        }
                    }
                }
            }
        }
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

// ── 8×8 Bitmap font (printable ASCII 32–126) ─────────────────────────────────
//
// Standard VGA/PC 8×8 font data for printable ASCII characters.
// Each entry is 8 bytes (one byte per row, bit 7 = leftmost pixel).

fn get_glyph(ch: char) -> [u8; 8] {
    let idx = match ch as u32 {
        32..=126 => (ch as usize) - 32,
        _ => 0, // space for unknown
    };
    FONT_8X8[idx]
}

#[rustfmt::skip]
static FONT_8X8: [[u8; 8]; 95] = [
    // 32 SPACE
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00],
    // 33 !
    [0x18,0x3C,0x3C,0x18,0x18,0x00,0x18,0x00],
    // 34 "
    [0x36,0x36,0x00,0x00,0x00,0x00,0x00,0x00],
    // 35 #
    [0x36,0x36,0x7F,0x36,0x7F,0x36,0x36,0x00],
    // 36 $
    [0x0C,0x3E,0x03,0x1E,0x30,0x1F,0x0C,0x00],
    // 37 %
    [0x00,0x63,0x33,0x18,0x0C,0x66,0x63,0x00],
    // 38 &
    [0x1C,0x36,0x1C,0x6E,0x3B,0x33,0x6E,0x00],
    // 39 '
    [0x06,0x06,0x03,0x00,0x00,0x00,0x00,0x00],
    // 40 (
    [0x18,0x0C,0x06,0x06,0x06,0x0C,0x18,0x00],
    // 41 )
    [0x06,0x0C,0x18,0x18,0x18,0x0C,0x06,0x00],
    // 42 *
    [0x00,0x66,0x3C,0xFF,0x3C,0x66,0x00,0x00],
    // 43 +
    [0x00,0x0C,0x0C,0x3F,0x0C,0x0C,0x00,0x00],
    // 44 ,
    [0x00,0x00,0x00,0x00,0x00,0x0C,0x0C,0x06],
    // 45 -
    [0x00,0x00,0x00,0x3F,0x00,0x00,0x00,0x00],
    // 46 .
    [0x00,0x00,0x00,0x00,0x00,0x0C,0x0C,0x00],
    // 47 /
    [0x60,0x30,0x18,0x0C,0x06,0x03,0x01,0x00],
    // 48 0
    [0x3E,0x63,0x73,0x7B,0x6F,0x67,0x3E,0x00],
    // 49 1
    [0x0C,0x0E,0x0C,0x0C,0x0C,0x0C,0x3F,0x00],
    // 50 2
    [0x1E,0x33,0x30,0x1C,0x06,0x33,0x3F,0x00],
    // 51 3
    [0x1E,0x33,0x30,0x1C,0x30,0x33,0x1E,0x00],
    // 52 4
    [0x38,0x3C,0x36,0x33,0x7F,0x30,0x78,0x00],
    // 53 5
    [0x3F,0x03,0x1F,0x30,0x30,0x33,0x1E,0x00],
    // 54 6
    [0x1C,0x06,0x03,0x1F,0x33,0x33,0x1E,0x00],
    // 55 7
    [0x3F,0x33,0x30,0x18,0x0C,0x0C,0x0C,0x00],
    // 56 8
    [0x1E,0x33,0x33,0x1E,0x33,0x33,0x1E,0x00],
    // 57 9
    [0x1E,0x33,0x33,0x3E,0x30,0x18,0x0E,0x00],
    // 58 :
    [0x00,0x0C,0x0C,0x00,0x00,0x0C,0x0C,0x00],
    // 59 ;
    [0x00,0x0C,0x0C,0x00,0x00,0x0C,0x0C,0x06],
    // 60 <
    [0x18,0x0C,0x06,0x03,0x06,0x0C,0x18,0x00],
    // 61 =
    [0x00,0x00,0x3F,0x00,0x00,0x3F,0x00,0x00],
    // 62 >
    [0x06,0x0C,0x18,0x30,0x18,0x0C,0x06,0x00],
    // 63 ?
    [0x1E,0x33,0x30,0x18,0x0C,0x00,0x0C,0x00],
    // 64 @
    [0x3E,0x63,0x7B,0x7B,0x7B,0x03,0x1E,0x00],
    // 65 A
    [0x0C,0x1E,0x33,0x33,0x3F,0x33,0x33,0x00],
    // 66 B
    [0x3F,0x66,0x66,0x3E,0x66,0x66,0x3F,0x00],
    // 67 C
    [0x3C,0x66,0x03,0x03,0x03,0x66,0x3C,0x00],
    // 68 D
    [0x1F,0x36,0x66,0x66,0x66,0x36,0x1F,0x00],
    // 69 E
    [0x7F,0x46,0x16,0x1E,0x16,0x46,0x7F,0x00],
    // 70 F
    [0x7F,0x46,0x16,0x1E,0x16,0x06,0x0F,0x00],
    // 71 G
    [0x3C,0x66,0x03,0x03,0x73,0x66,0x7C,0x00],
    // 72 H
    [0x33,0x33,0x33,0x3F,0x33,0x33,0x33,0x00],
    // 73 I
    [0x1E,0x0C,0x0C,0x0C,0x0C,0x0C,0x1E,0x00],
    // 74 J
    [0x78,0x30,0x30,0x30,0x33,0x33,0x1E,0x00],
    // 75 K
    [0x67,0x66,0x36,0x1E,0x36,0x66,0x67,0x00],
    // 76 L
    [0x0F,0x06,0x06,0x06,0x46,0x66,0x7F,0x00],
    // 77 M
    [0x63,0x77,0x7F,0x7F,0x6B,0x63,0x63,0x00],
    // 78 N
    [0x63,0x67,0x6F,0x7B,0x73,0x63,0x63,0x00],
    // 79 O
    [0x1C,0x36,0x63,0x63,0x63,0x36,0x1C,0x00],
    // 80 P
    [0x3F,0x66,0x66,0x3E,0x06,0x06,0x0F,0x00],
    // 81 Q
    [0x1E,0x33,0x33,0x33,0x3B,0x1E,0x38,0x00],
    // 82 R
    [0x3F,0x66,0x66,0x3E,0x36,0x66,0x67,0x00],
    // 83 S
    [0x1E,0x33,0x07,0x0E,0x38,0x33,0x1E,0x00],
    // 84 T
    [0x3F,0x2D,0x0C,0x0C,0x0C,0x0C,0x1E,0x00],
    // 85 U
    [0x33,0x33,0x33,0x33,0x33,0x33,0x3F,0x00],
    // 86 V
    [0x33,0x33,0x33,0x33,0x33,0x1E,0x0C,0x00],
    // 87 W
    [0x63,0x63,0x63,0x6B,0x7F,0x77,0x63,0x00],
    // 88 X
    [0x63,0x63,0x36,0x1C,0x1C,0x36,0x63,0x00],
    // 89 Y
    [0x33,0x33,0x33,0x1E,0x0C,0x0C,0x1E,0x00],
    // 90 Z
    [0x7F,0x63,0x31,0x18,0x4C,0x66,0x7F,0x00],
    // 91 [
    [0x1E,0x06,0x06,0x06,0x06,0x06,0x1E,0x00],
    // 92 backslash
    [0x03,0x06,0x0C,0x18,0x30,0x60,0x40,0x00],
    // 93 ]
    [0x1E,0x18,0x18,0x18,0x18,0x18,0x1E,0x00],
    // 94 ^
    [0x08,0x1C,0x36,0x63,0x00,0x00,0x00,0x00],
    // 95 _
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xFF],
    // 96 `
    [0x0C,0x0C,0x18,0x00,0x00,0x00,0x00,0x00],
    // 97 a
    [0x00,0x00,0x1E,0x30,0x3E,0x33,0x6E,0x00],
    // 98 b
    [0x07,0x06,0x06,0x3E,0x66,0x66,0x3B,0x00],
    // 99 c
    [0x00,0x00,0x1E,0x33,0x03,0x33,0x1E,0x00],
    // 100 d
    [0x38,0x30,0x30,0x3E,0x33,0x33,0x6E,0x00],
    // 101 e
    [0x00,0x00,0x1E,0x33,0x3F,0x03,0x1E,0x00],
    // 102 f
    [0x1C,0x36,0x06,0x0F,0x06,0x06,0x0F,0x00],
    // 103 g
    [0x00,0x00,0x6E,0x33,0x33,0x3E,0x30,0x1F],
    // 104 h
    [0x07,0x06,0x36,0x6E,0x66,0x66,0x67,0x00],
    // 105 i
    [0x0C,0x00,0x0E,0x0C,0x0C,0x0C,0x1E,0x00],
    // 106 j
    [0x30,0x00,0x30,0x30,0x30,0x33,0x33,0x1E],
    // 107 k
    [0x07,0x06,0x66,0x36,0x1E,0x36,0x67,0x00],
    // 108 l
    [0x0E,0x0C,0x0C,0x0C,0x0C,0x0C,0x1E,0x00],
    // 109 m
    [0x00,0x00,0x33,0x7F,0x7F,0x6B,0x63,0x00],
    // 110 n
    [0x00,0x00,0x1F,0x33,0x33,0x33,0x33,0x00],
    // 111 o
    [0x00,0x00,0x1E,0x33,0x33,0x33,0x1E,0x00],
    // 112 p
    [0x00,0x00,0x3B,0x66,0x66,0x3E,0x06,0x0F],
    // 113 q
    [0x00,0x00,0x6E,0x33,0x33,0x3E,0x30,0x78],
    // 114 r
    [0x00,0x00,0x3B,0x6E,0x66,0x06,0x0F,0x00],
    // 115 s
    [0x00,0x00,0x3E,0x03,0x1E,0x30,0x1F,0x00],
    // 116 t
    [0x08,0x0C,0x3E,0x0C,0x0C,0x2C,0x18,0x00],
    // 117 u
    [0x00,0x00,0x33,0x33,0x33,0x33,0x6E,0x00],
    // 118 v
    [0x00,0x00,0x33,0x33,0x33,0x1E,0x0C,0x00],
    // 119 w
    [0x00,0x00,0x63,0x6B,0x7F,0x7F,0x36,0x00],
    // 120 x
    [0x00,0x00,0x63,0x36,0x1C,0x36,0x63,0x00],
    // 121 y
    [0x00,0x00,0x33,0x33,0x33,0x3E,0x30,0x1F],
    // 122 z
    [0x00,0x00,0x3F,0x19,0x0C,0x26,0x3F,0x00],
    // 123 {
    [0x38,0x0C,0x0C,0x07,0x0C,0x0C,0x38,0x00],
    // 124 |
    [0x18,0x18,0x18,0x00,0x18,0x18,0x18,0x00],
    // 125 }
    [0x07,0x0C,0x0C,0x38,0x0C,0x0C,0x07,0x00],
    // 126 ~
    [0x6E,0x3B,0x00,0x00,0x00,0x00,0x00,0x00],
];
