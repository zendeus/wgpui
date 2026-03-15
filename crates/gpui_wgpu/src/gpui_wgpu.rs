mod cosmic_text_system;
mod gpu_canvas_composite;
mod wgpu_atlas;
mod wgpu_context;
mod wgpu_renderer;

use std::sync::Arc;

pub use cosmic_text_system::*;
pub use wgpu;
pub use wgpu_atlas::*;
pub use wgpu_context::*;
pub use wgpu_renderer::{
    GpuCanvasCallback, GpuCanvasContext, GpuContext, WgpuRenderer, WgpuSurfaceConfig,
};

/// Create a typed gpu_canvas callback wrapped as a type-erased `GpuCanvasCallback`
/// suitable for passing to `gpui::gpu_canvas()`.
///
/// ```ignore
/// use gpui_wgpu::gpu_canvas_callback;
/// use gpui::gpu_canvas;
///
/// gpu_canvas(gpu_canvas_callback(|ctx| {
///     // ctx.device, ctx.queue, ctx.target, etc.
/// }))
/// .w(px(400.0))
/// .h(px(400.0))
/// ```
pub fn gpu_canvas_callback(
    callback: impl Fn(&mut GpuCanvasContext) + Send + Sync + 'static,
) -> gpui::GpuCanvasCallback {
    let typed: GpuCanvasCallback = Arc::new(callback);
    Arc::new(typed)
}
