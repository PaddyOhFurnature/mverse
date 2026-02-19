//! WGSL Shaders

/// Basic vertex/fragment shader for colored geometry with MVP transform and simple lighting
pub const BASIC_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
}

struct Uniforms {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    out.normal = in.normal;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Simple directional lighting
    let light_dir = normalize(vec3<f32>(0.5, -0.7, -0.5)); // Sun from above/side
    let ambient = 0.3; // Ambient light level
    let normal = normalize(in.normal);
    
    // Diffuse lighting (Lambertian)
    let diffuse = max(dot(-light_dir, normal), 0.0);
    
    // Combine ambient + diffuse
    let lighting = ambient + (1.0 - ambient) * diffuse;
    
    // Apply lighting to color
    return vec4<f32>(in.color.rgb * lighting, in.color.a);
}
"#;
