// Vertex shader
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
    @location(1) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    
    // Apply model transform, then camera transform
    let world_pos = model.model * vec4<f32>(in.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    
    // Transform normal by model matrix (ignoring translation)
    let world_normal = model.model * vec4<f32>(in.normal, 0.0);
    out.world_normal = world_normal.xyz;
    out.world_position = world_pos.xyz;
    
    return out;
}

// Fragment shader
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Simple directional lighting
    let light_dir = normalize(vec3<f32>(0.5, 0.8, 0.3));
    let ambient = 0.3;
    
    let normal = normalize(in.world_normal);
    let diffuse = max(dot(normal, light_dir), 0.0);
    let lighting = ambient + (1.0 - ambient) * diffuse;
    
    // Base color (gray stone)
    let base_color = vec3<f32>(0.6, 0.6, 0.6);
    let final_color = base_color * lighting;
    
    return vec4<f32>(final_color, 1.0);
}
