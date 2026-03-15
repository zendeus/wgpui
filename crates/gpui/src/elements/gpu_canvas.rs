use refineable::Refineable as _;

use crate::{
    App, Bounds, Element, ElementId, GpuCanvasCallback, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Style, StyleRefinement, Styled, Window,
};

/// Create a gpu_canvas element with a type-erased rendering callback.
///
/// The callback is stored as `Arc<dyn Any + Send + Sync>` and will be downcast
/// by the renderer to the concrete callback type at draw time. Use the typed
/// constructor from `gpui_wgpu` for a better API.
pub fn gpu_canvas(callback: GpuCanvasCallback) -> GpuCanvas {
    GpuCanvas {
        callback,
        style: StyleRefinement::default(),
    }
}

/// An element that lets users render arbitrary GPU content into a compositable UI element.
pub struct GpuCanvas {
    callback: GpuCanvasCallback,
    style: StyleRefinement,
}

impl Styled for GpuCanvas {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for GpuCanvas {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for GpuCanvas {
    type RequestLayoutState = Style;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style.clone(), [], cx);
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Style,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _style: &mut Style,
        _prepaint: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        window.paint_gpu_canvas(bounds, self.callback.clone());
    }
}
