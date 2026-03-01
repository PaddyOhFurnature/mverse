// Vertex shader for OSM objects (buildings, roads, water).
// The vertex "normal" slot carries RGB colour, not a true geometric normal.
// This shader passes that colour straight through to the fragment, so buildings
// show the colour computed in Rust (beige, blue, grey, etc.) rather than the
// terrain grass/rock blend of the main shader.

struct CameraUniform {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct ModelUniform {
    model: mat4x4<f32>,
};

@group(1) @binding(0)
var<uniform> model: ModelUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,   // packed into the "normal" slot of Vertex
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model.model * vec4<f32>(in.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Simple sun-independent flat shading.  Slight ambient-only lighting so
    // night/day doesn't matter — we just want the correct OSM colours visible.
    return vec4<f32>(in.color, 1.0);
}
