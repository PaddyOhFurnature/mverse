// Textured shader for GLB models (buildings, world objects).
// Groups: 0=camera, 1=model-transform, 2=diffuse-texture+sampler.

struct CameraUniform {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct ModelUniform {
    model: mat4x4<f32>,
};
@group(1) @binding(0) var<uniform> model_u: ModelUniform;

@group(2) @binding(0) var t_diffuse: texture_2d<f32>;
@group(2) @binding(1) var s_diffuse: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv:     vec2<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model_u.model * vec4<f32>(in.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    out.uv = in.uv;
    let nm = mat3x3<f32>(
        model_u.model[0].xyz,
        model_u.model[1].xyz,
        model_u.model[2].xyz,
    );
    out.normal = normalize(nm * in.normal);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(t_diffuse, s_diffuse, in.uv);
    let sun = normalize(vec3<f32>(0.4, 1.0, 0.6));
    let diffuse = max(dot(in.normal, sun), 0.25);
    return vec4<f32>(tex.rgb * diffuse, tex.a);
}
