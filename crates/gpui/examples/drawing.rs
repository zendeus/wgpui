//! Example: simple freehand drawing application.
//!
//! Demonstrates mouse event handling (down/move/up) with gpu_canvas
//! for rendering strokes as line-segment quads.

use std::sync::{Arc, Mutex};

use gpui::{
    App, Bounds, Context, MouseButton, MouseDownEvent, MouseMoveEvent, MousePressureEvent,
    MouseUpEvent, Window, WindowBounds, WindowOptions, div, gpu_canvas, prelude::*, px, rgb, size,
};
use gpui_platform::application;
use gpui_wgpu::gpu_canvas_callback;
use wgpu::util::DeviceExt;

const MIN_LINE_WIDTH: f32 = 2.0;
const MAX_LINE_WIDTH: f32 = 20.0;
const DEFAULT_LINE_WIDTH: f32 = 10.0;

/// Inline WGSL shader for rendering line segments from a storage buffer.
const STROKE_SHADER: &str = r#"
struct Uniforms {
    viewport_size: vec2<f32>,
    scale_factor: f32,
    _pad: f32,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var<storage, read> vertices: array<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = vertices[vi * 2u];
    let y = vertices[vi * 2u + 1u];

    // Convert from logical pixels to device pixels, then to NDC
    let device_pos = vec2<f32>(x, y) * uniforms.scale_factor;
    let ndc = device_pos / uniforms.viewport_size * 2.0 - 1.0;
    return vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    // Solid dark color, premultiplied alpha
    return vec4<f32>(0.2, 0.2, 0.2, 1.0);
}
"#;

const MSAA_SAMPLE_COUNT: u32 = 4;

/// GPU resources cached across frames.
struct GpuState {
    pipeline: Option<wgpu::RenderPipeline>,
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    uniform_buffer: Option<wgpu::Buffer>,
    vertex_buffer: Option<wgpu::Buffer>,
    vertex_buffer_capacity: usize,
    /// MSAA render target — created/recreated when canvas size changes.
    msaa_texture: Option<wgpu::TextureView>,
    msaa_texture_size: (u32, u32),
}

/// Normalize a 2D vector, returning a fallback if too short.
fn normalize(v: [f32; 2]) -> [f32; 2] {
    let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if len < 0.001 {
        [1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len]
    }
}

/// A stroke point with position and pressure-derived half-width.
#[derive(Clone, Copy)]
struct StrokePoint {
    pos: [f32; 2],
    half_width: f32,
}

/// Compute half-width from pressure (0.0–1.0). Falls back to default when no pressure data.
fn half_width_from_pressure(pressure: f32) -> f32 {
    if pressure <= 0.0 {
        DEFAULT_LINE_WIDTH / 2.0
    } else {
        (MIN_LINE_WIDTH + pressure * (MAX_LINE_WIDTH - MIN_LINE_WIDTH)) / 2.0
    }
}

/// Tessellate a polyline into a connected filled hull.
/// Builds left/right outlines with averaged normals at each point,
/// then tessellates as a triangle strip. Round caps at endpoints.
fn tessellate_stroke(points: &[StrokePoint]) -> Vec<f32> {
    if points.len() < 2 {
        return Vec::new();
    }

    let n = points.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);

    for i in 0..n {
        let hw = points[i].half_width;
        let p = points[i].pos;

        let dir = if i == 0 {
            normalize([points[1].pos[0] - p[0], points[1].pos[1] - p[1]])
        } else if i == n - 1 {
            normalize([p[0] - points[i - 1].pos[0], p[1] - points[i - 1].pos[1]])
        } else {
            let d1 = normalize([
                p[0] - points[i - 1].pos[0],
                p[1] - points[i - 1].pos[1],
            ]);
            let d2 = normalize([
                points[i + 1].pos[0] - p[0],
                points[i + 1].pos[1] - p[1],
            ]);
            normalize([d1[0] + d2[0], d1[1] + d2[1]])
        };

        let nx = -dir[1] * hw;
        let ny = dir[0] * hw;

        left.push([p[0] + nx, p[1] + ny]);
        right.push([p[0] - nx, p[1] - ny]);
    }

    let mut verts = Vec::with_capacity((n - 1) * 12 + 128);
    for i in 0..n - 1 {
        verts.extend_from_slice(&[
            left[i][0], left[i][1], right[i][0], right[i][1], left[i + 1][0], left[i + 1][1],
        ]);
        verts.extend_from_slice(&[
            right[i][0], right[i][1], right[i + 1][0], right[i + 1][1], left[i + 1][0],
            left[i + 1][1],
        ]);
    }

    // Round end caps
    let cap_segments = 8;
    let start_hw = points[0].half_width;
    let start_dir = normalize([
        points[1].pos[0] - points[0].pos[0],
        points[1].pos[1] - points[0].pos[1],
    ]);
    let start_angle = start_dir[1].atan2(start_dir[0]);
    for s in 0..cap_segments {
        let a0 = start_angle + std::f32::consts::PI * 0.5
            + std::f32::consts::PI * s as f32 / cap_segments as f32;
        let a1 = start_angle + std::f32::consts::PI * 0.5
            + std::f32::consts::PI * (s + 1) as f32 / cap_segments as f32;
        verts.extend_from_slice(&[
            points[0].pos[0],
            points[0].pos[1],
            points[0].pos[0] + a0.cos() * start_hw,
            points[0].pos[1] + a0.sin() * start_hw,
            points[0].pos[0] + a1.cos() * start_hw,
            points[0].pos[1] + a1.sin() * start_hw,
        ]);
    }
    let end_hw = points[n - 1].half_width;
    let end_dir = normalize([
        points[n - 1].pos[0] - points[n - 2].pos[0],
        points[n - 1].pos[1] - points[n - 2].pos[1],
    ]);
    let end_angle = end_dir[1].atan2(end_dir[0]);
    for s in 0..cap_segments {
        let a0 = end_angle - std::f32::consts::PI * 0.5
            + std::f32::consts::PI * s as f32 / cap_segments as f32;
        let a1 = end_angle - std::f32::consts::PI * 0.5
            + std::f32::consts::PI * (s + 1) as f32 / cap_segments as f32;
        verts.extend_from_slice(&[
            points[n - 1].pos[0],
            points[n - 1].pos[1],
            points[n - 1].pos[0] + a0.cos() * end_hw,
            points[n - 1].pos[1] + a0.sin() * end_hw,
            points[n - 1].pos[0] + a1.cos() * end_hw,
            points[n - 1].pos[1] + a1.sin() * end_hw,
        ]);
    }

    verts
}

struct DrawingApp {
    finished_strokes: Vec<Vec<StrokePoint>>,
    cached_verts: Arc<Vec<f32>>,
    active_stroke: Option<Vec<StrokePoint>>,
    active_verts: Arc<Vec<f32>>,
    current_pressure: f32,
    gpu_state: Arc<Mutex<GpuState>>,
}

impl Render for DrawingApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(ref active) = self.active_stroke {
            self.active_verts = Arc::new(tessellate_stroke(active));
        } else if !self.active_verts.is_empty() {
            self.active_verts = Arc::new(Vec::new());
        }

        let cached = self.cached_verts.clone();
        let active = self.active_verts.clone();
        let total_vertex_count = (cached.len() + active.len()) / 2;
        let gpu_state = self.gpu_state.clone();

        let callback = gpu_canvas_callback(move |ctx: &mut gpui_wgpu::GpuCanvasContext| {
            if total_vertex_count == 0 {
                let mut encoder =
                    ctx.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("clear_encoder"),
                        });
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("clear_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: ctx.target,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: None,
                        ..Default::default()
                    });
                }
                ctx.queue.submit([encoder.finish()]);
                return;
            }

            let mut state = gpu_state.lock().expect("lock poisoned");

            if state.pipeline.is_none() {
                let shader = ctx
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("stroke_shader"),
                        source: wgpu::ShaderSource::Wgsl(STROKE_SHADER.into()),
                    });

                let bind_group_layout =
                    ctx.device
                        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                            label: Some("stroke_bgl"),
                            entries: &[
                                wgpu::BindGroupLayoutEntry {
                                    binding: 0,
                                    visibility: wgpu::ShaderStages::VERTEX,
                                    ty: wgpu::BindingType::Buffer {
                                        ty: wgpu::BufferBindingType::Uniform,
                                        has_dynamic_offset: false,
                                        min_binding_size: None,
                                    },
                                    count: None,
                                },
                                wgpu::BindGroupLayoutEntry {
                                    binding: 1,
                                    visibility: wgpu::ShaderStages::VERTEX,
                                    ty: wgpu::BindingType::Buffer {
                                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                                        has_dynamic_offset: false,
                                        min_binding_size: None,
                                    },
                                    count: None,
                                },
                            ],
                        });

                let pipeline_layout =
                    ctx.device
                        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                            label: Some("stroke_pipeline_layout"),
                            bind_group_layouts: &[&bind_group_layout],
                            immediate_size: 0,
                        });

                let pipeline =
                    ctx.device
                        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                            label: Some("stroke_pipeline"),
                            layout: Some(&pipeline_layout),
                            vertex: wgpu::VertexState {
                                module: &shader,
                                entry_point: Some("vs_main"),
                                compilation_options: Default::default(),
                                buffers: &[],
                            },
                            fragment: Some(wgpu::FragmentState {
                                module: &shader,
                                entry_point: Some("fs_main"),
                                compilation_options: Default::default(),
                                targets: &[Some(wgpu::ColorTargetState {
                                    format: ctx.target_format,
                                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                                    write_mask: wgpu::ColorWrites::ALL,
                                })],
                            }),
                            primitive: wgpu::PrimitiveState::default(),
                            depth_stencil: None,
                            multisample: wgpu::MultisampleState {
                                count: MSAA_SAMPLE_COUNT,
                                mask: !0,
                                alpha_to_coverage_enabled: false,
                            },
                            multiview_mask: None,
                            cache: None,
                        });

                state.bind_group_layout = Some(bind_group_layout);
                state.pipeline = Some(pipeline);
            }

            let uniforms: [f32; 4] = [
                ctx.size.width.0 as f32,
                ctx.size.height.0 as f32,
                ctx.scale_factor,
                0.0,
            ];

            match &state.uniform_buffer {
                Some(buf) => {
                    ctx.queue
                        .write_buffer(buf, 0, bytemuck::cast_slice(&uniforms))
                }
                None => {
                    state.uniform_buffer = Some(ctx.device.create_buffer_init(
                        &wgpu::util::BufferInitDescriptor {
                            label: Some("stroke_uniforms"),
                            contents: bytemuck::cast_slice(&uniforms),
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        },
                    ));
                }
            }

            let total_floats = cached.len() + active.len();
            let needs_new_buffer =
                total_floats > state.vertex_buffer_capacity || state.vertex_buffer.is_none();

            if needs_new_buffer {
                let new_capacity = total_floats.max(1024).next_power_of_two();
                let mut data = Vec::with_capacity(new_capacity);
                data.extend_from_slice(&cached);
                data.extend_from_slice(&active);
                data.resize(new_capacity, 0.0);

                state.vertex_buffer = Some(ctx.device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("stroke_vertices"),
                        contents: bytemuck::cast_slice(&data),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    },
                ));
                state.vertex_buffer_capacity = new_capacity;
            } else if let Some(ref buf) = state.vertex_buffer {
                ctx.queue
                    .write_buffer(buf, 0, bytemuck::cast_slice(&cached));
                if !active.is_empty() {
                    ctx.queue.write_buffer(
                        buf,
                        (cached.len() * std::mem::size_of::<f32>()) as u64,
                        bytemuck::cast_slice(&active),
                    );
                }
            }

            let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("stroke_bg"),
                layout: state.bind_group_layout.as_ref().expect("layout exists"),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: state
                            .uniform_buffer
                            .as_ref()
                            .expect("uniform exists")
                            .as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: state
                            .vertex_buffer
                            .as_ref()
                            .expect("vertex exists")
                            .as_entire_binding(),
                    },
                ],
            });

            // Create or recreate MSAA texture if size changed
            let current_size = (ctx.size.width.0 as u32, ctx.size.height.0 as u32);
            if state.msaa_texture.is_none() || state.msaa_texture_size != current_size {
                let msaa_texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("msaa_texture"),
                    size: wgpu::Extent3d {
                        width: current_size.0.max(1),
                        height: current_size.1.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: MSAA_SAMPLE_COUNT,
                    dimension: wgpu::TextureDimension::D2,
                    format: ctx.target_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                state.msaa_texture = Some(msaa_texture.create_view(&Default::default()));
                state.msaa_texture_size = current_size;
            }
            let msaa_view = state.msaa_texture.as_ref().expect("msaa texture exists");

            let mut encoder =
                ctx.device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("stroke_encoder"),
                    });

            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("stroke_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: msaa_view,
                        resolve_target: Some(ctx.target),
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Discard,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    ..Default::default()
                });

                pass.set_pipeline(state.pipeline.as_ref().expect("pipeline exists"));
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..total_vertex_count as u32, 0..1);
            }

            ctx.queue.submit([encoder.finish()]);
        });

        div()
            .id("canvas")
            .size_full()
            .bg(rgb(0xffffff))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, _cx| {
                    this.active_stroke = Some(vec![StrokePoint {
                        pos: [f32::from(event.position.x), f32::from(event.position.y)],
                        half_width: half_width_from_pressure(this.current_pressure),
                    }]);
                }),
            )
            .on_mouse_move(cx.listener(
                |this, event: &MouseMoveEvent, _window, _cx| {
                    if let Some(ref mut stroke) = this.active_stroke {
                        stroke.push(StrokePoint {
                            pos: [f32::from(event.position.x), f32::from(event.position.y)],
                            half_width: half_width_from_pressure(this.current_pressure),
                        });
                    }
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, _cx| {
                    let _ = event;
                    if let Some(stroke) = this.active_stroke.take() {
                        let mut new_cached = (*this.cached_verts).clone();
                        new_cached.extend(tessellate_stroke(&stroke));
                        this.cached_verts = Arc::new(new_cached);
                        this.finished_strokes.push(stroke);
                        this.active_verts = Arc::new(Vec::new());
                    }
                    this.current_pressure = 0.0;
                }),
            )
            .on_mouse_pressure(cx.listener(
                |this, event: &MousePressureEvent, _window, _cx| {
                    this.current_pressure = event.pressure;
                },
            ))
            .child(gpu_canvas(callback).size_full())
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.), px(600.0)), cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| DrawingApp {
                    finished_strokes: Vec::new(),
                    cached_verts: Arc::new(Vec::new()),
                    active_stroke: None,
                    active_verts: Arc::new(Vec::new()),
                    current_pressure: 0.0,
                    gpu_state: Arc::new(Mutex::new(GpuState {
                        pipeline: None,
                        bind_group_layout: None,
                        uniform_buffer: None,
                        vertex_buffer: None,
                        vertex_buffer_capacity: 0,
                        msaa_texture: None,
                        msaa_texture_size: (0, 0),
                    })),
                })
            },
        )
        .unwrap();

        cx.activate(true);
    });
}
