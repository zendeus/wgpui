// Compositing shader for gpu_canvas offscreen textures.
// Renders a screen-space quad that samples the offscreen texture,
// clipped by the content mask.

struct Uniforms {
    viewport_size: vec2<f32>,
    bounds_origin: vec2<f32>,
    bounds_size: vec2<f32>,
    content_mask_origin: vec2<f32>,
    content_mask_size: vec2<f32>,
    _pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var canvas_texture: texture_2d<f32>;
@group(0) @binding(2) var canvas_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) pixel_position: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Generate a full-screen quad from vertex_index (triangle strip: 0,1,2,3)
    let uv = vec2<f32>(
        f32(vertex_index & 1u),
        f32((vertex_index >> 1u) & 1u),
    );

    // Map UV to the element bounds in pixel coordinates
    let pixel_pos = uniforms.bounds_origin + uv * uniforms.bounds_size;

    // Convert pixel position to clip space: [0, viewport] -> [-1, 1]
    let ndc = vec2<f32>(
        pixel_pos.x / uniforms.viewport_size.x * 2.0 - 1.0,
        1.0 - pixel_pos.y / uniforms.viewport_size.y * 2.0,
    );

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = uv;
    out.pixel_position = pixel_pos;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Content mask clipping
    let mask_min = uniforms.content_mask_origin;
    let mask_max = uniforms.content_mask_origin + uniforms.content_mask_size;
    if in.pixel_position.x < mask_min.x || in.pixel_position.x > mask_max.x ||
       in.pixel_position.y < mask_min.y || in.pixel_position.y > mask_max.y {
        discard;
    }

    return textureSample(canvas_texture, canvas_sampler, in.uv);
}
