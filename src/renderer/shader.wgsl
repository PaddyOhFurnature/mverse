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
    @location(2) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) vert_color: vec3<f32>,
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
    out.vert_color = in.color;
    
    return out;
}

// Fragment shader
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(in.world_normal);

    // Slope factor: 1 = flat top, 0 = vertical wall
    let up_dot = dot(normal, vec3<f32>(0.0, 1.0, 0.0));

    // Vertex color carries material color from terrain generation (asphalt, grass, rock etc.)
    // On steep slopes, blend toward a rock color so cliff faces look natural regardless of material.
    let rock = vec3<f32>(0.52, 0.47, 0.42);
    let t_rock = clamp((0.3 - up_dot) * 4.0, 0.0, 1.0);
    let base_color = mix(in.vert_color, rock, t_rock);

    // Directional sun light
    let light_dir = normalize(vec3<f32>(0.5, 1.0, 0.4));
    let ambient   = 0.40;
    let diffuse   = max(dot(normal, light_dir), 0.0);
    let lighting  = ambient + (1.0 - ambient) * diffuse;

    return vec4<f32>(base_color * lighting, 1.0);
}
