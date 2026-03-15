// Animated semi-transparent water surface shader.
// Group 0: camera  (shared with terrain pipeline)
// Group 1: model   (shared with terrain pipeline)
// Group 2: time    (water-specific, animation clock)

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

struct TimeUniform {
    time_secs: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};
@group(2) @binding(0)
var<uniform> time_u: TimeUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) world_xz: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model.model * vec4<f32>(in.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    out.color = in.color;
    out.world_xz = world_pos.xz;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = time_u.time_secs;

    // Two overlapping ripple waves at different scales and speeds
    let r1 = sin(in.world_xz.x * 0.12 + in.world_xz.y * 0.08 + t * 0.7) * 0.5 + 0.5;
    let r2 = sin(in.world_xz.x * 0.07 - in.world_xz.y * 0.11 + t * 1.1) * 0.5 + 0.5;
    let ripple = r1 * 0.55 + r2 * 0.45;

    // Subtle brightness modulation from ripple
    let brightness = 0.82 + ripple * 0.18;
    let water_color = in.color * brightness;

    // Semi-transparent: alpha 0.72 gives clear view of riverbed through shallows
    return vec4<f32>(water_color, 0.72);
}
