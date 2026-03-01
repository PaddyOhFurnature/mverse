//! Basic renderer for terrain visualization
//!
//! Minimal wgpu-based renderer for validation purposes.
//! NOT production quality - just enough to see if terrain generation works.

mod camera;
mod pipeline;
mod textured_pipeline;

pub use camera::Camera;
pub use pipeline::{OsmPipeline, RenderContext, RenderPipeline};
pub use textured_pipeline::{GlbModel, TexturedPipeline, TexturedVertex};

use crate::mesh::Mesh;
use glam::Mat4;
use wgpu::util::DeviceExt;

/// GPU buffer containing mesh data
pub struct MeshBuffer {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
}

impl MeshBuffer {
    /// Upload mesh to GPU
    pub fn from_mesh(device: &wgpu::Device, mesh: &Mesh) -> Self {
        // Convert mesh vertices to GPU format
        let vertices: Vec<Vertex> = mesh
            .vertices
            .iter()
            .map(|v| Vertex {
                position: [v.position.x, v.position.y, v.position.z],
                normal: [v.normal.x, v.normal.y, v.normal.z],
            })
            .collect();

        // Convert triangle indices to u32
        let indices: Vec<u32> = mesh
            .triangles
            .iter()
            .flat_map(|tri| tri.indices.iter().map(|&i| i as u32))
            .collect();

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mesh Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            vertex_buffer,
            index_buffer,
            num_indices: indices.len() as u32,
        }
    }

    /// Render this mesh buffer
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.num_indices, 0, 0..1);
    }
}

/// Vertex format for GPU
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// Uniform buffer for camera matrices
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    fn new() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
        }
    }

    fn update(&mut self, camera: &Camera) {
        self.view_proj = camera.build_view_projection_matrix().to_cols_array_2d();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertex_size() {
        assert_eq!(std::mem::size_of::<Vertex>(), 24); // 3 floats + 3 floats
    }

    #[test]
    fn test_camera_uniform_size() {
        assert_eq!(std::mem::size_of::<CameraUniform>(), 64); // 4x4 matrix
    }
}
