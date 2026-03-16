// Compositing shader for gpu_canvas offscreen textures.
// Renders a screen-space quad that samples the offscreen texture,
// clipped by the content mask.

struct Uniforms {
    viewport_size: vec2<f32>,
    bounds_origin: vec2<f32>,
    bounds_size: vec2<f32>,
    content_mask_origin: vec2<f32>,
    content_mask_size: vec2<f32>,
    content_mask_corner_radii: vec4<f32>,
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

// Inline SDF for rounded rect clipping (standalone shader, no access to shaders.wgsl functions)
fn pick_corner_radius_composite(center_to_point: vec2<f32>, radii: vec4<f32>) -> f32 {
    // radii: (top_left, top_right, bottom_right, bottom_left)
    if (center_to_point.x < 0.0) {
        if (center_to_point.y < 0.0) {
            return radii.x; // top_left
        } else {
            return radii.w; // bottom_left
        }
    } else {
        if (center_to_point.y < 0.0) {
            return radii.y; // top_right
        } else {
            return radii.z; // bottom_right
        }
    }
}

fn rounded_rect_sdf(point: vec2<f32>, origin: vec2<f32>, size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let half_size = size / 2.0;
    let center = origin + half_size;
    let center_to_point = point - center;
    let corner_radius = pick_corner_radius_composite(center_to_point, radii);
    let corner_to_point = abs(center_to_point) - half_size;
    let corner_center_to_point = corner_to_point + corner_radius;
    if (corner_radius == 0.0) {
        return max(corner_center_to_point.x, corner_center_to_point.y);
    } else {
        let signed_distance =
            length(max(vec2<f32>(0.0), corner_center_to_point)) +
            min(0.0, max(corner_center_to_point.x, corner_center_to_point.y));
        return signed_distance - corner_radius;
    }
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Content mask clipping (AABB)
    let mask_min = uniforms.content_mask_origin;
    let mask_max = uniforms.content_mask_origin + uniforms.content_mask_size;
    if in.pixel_position.x < mask_min.x || in.pixel_position.x > mask_max.x ||
       in.pixel_position.y < mask_min.y || in.pixel_position.y > mask_max.y {
        discard;
    }

    // Rounded content mask clipping
    let radii = uniforms.content_mask_corner_radii;
    if radii.x != 0.0 || radii.y != 0.0 || radii.z != 0.0 || radii.w != 0.0 {
        let d = rounded_rect_sdf(in.pixel_position, uniforms.content_mask_origin, uniforms.content_mask_size, radii);
        let mask_alpha = 1.0 - smoothstep(-0.5, 0.5, d);
        if mask_alpha <= 0.0 { discard; }
        let color = textureSample(canvas_texture, canvas_sampler, in.uv);
        return vec4<f32>(color.rgb * mask_alpha, color.a * mask_alpha);
    }

    return textureSample(canvas_texture, canvas_sampler, in.uv);
}
