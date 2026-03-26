use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    mem,
};

use scheduler::Instant;

use crate::{
    AnyElement, Animation, App, Bounds, ContentMask, Corners, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, SharedString, Size, Style, Window,
};

/// Identifies a matched geometry pair across the element tree.
/// Two elements sharing the same `MatchedGeometryId` will animate between
/// each other's bounds when one replaces the other.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct MatchedGeometryId {
    /// The namespace grouping related matched geometry pairs.
    pub namespace: ElementId,
    /// The specific identity within the namespace.
    pub id: ElementId,
}

/// Entry stored per-frame in the window's matched geometry map.
#[derive(Clone, Debug)]
pub struct MatchedGeometryEntry {
    /// The computed bounds of the element in this frame.
    pub bounds: Bounds<Pixels>,
    /// The element's natural target bounds (without animation wrapper influence).
    /// `Some` when the element is currently animating; `None` otherwise.
    pub natural_bounds: Option<Bounds<Pixels>>,
}

/// Per-element animation state persisted across frames via `with_element_state`.
struct MatchedGeometryAnimState {
    start: Instant,
    from_bounds: Bounds<Pixels>,
    to_bounds: Bounds<Pixels>,
}

/// A wrapper element that participates in matched geometry transitions.
///
/// Registers its bounds each frame. When this element appears at a different
/// position or size than the previous frame's entry with the same ID, it smoothly
/// animates both position and size using deferred drawing with a content mask.
///
/// For collapse (big → small), on the first frame the natural target bounds are
/// captured. On subsequent frames a wrapper layout forces the element to render at
/// the previous interpolated size, so the content mask can clip it down smoothly.
/// For best collapse results, use flexible sizing (`size_full()`) on matched elements.
pub struct MatchedGeometryElement<E> {
    element_id: ElementId,
    matched_id: MatchedGeometryId,
    element: Option<E>,
    animation: Animation,
    corner_radii: Corners<Pixels>,
}

impl<E> MatchedGeometryElement<E> {
    /// Set the corner radii used for clipping during the size transition.
    /// Should match the element's visual border radius for a seamless animation.
    pub fn with_corner_radii(mut self, radius: Pixels) -> Self {
        self.corner_radii = Corners::all(radius);
        self
    }
}

fn make_element_id(namespace: &ElementId, id: &ElementId) -> ElementId {
    let mut hasher = DefaultHasher::new();
    namespace.hash(&mut hasher);
    id.hash(&mut hasher);
    ElementId::NamedInteger(SharedString::from("matched-geo"), hasher.finish())
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_bounds(from: &Bounds<Pixels>, to: &Bounds<Pixels>, t: f32) -> Bounds<Pixels> {
    Bounds {
        origin: Point {
            x: Pixels(lerp(from.origin.x.0, to.origin.x.0, t)),
            y: Pixels(lerp(from.origin.y.0, to.origin.y.0, t)),
        },
        size: Size {
            width: Pixels(lerp(from.size.width.0, to.size.width.0, t)),
            height: Pixels(lerp(from.size.height.0, to.size.height.0, t)),
        },
    }
}

fn bounds_differ(a: &Bounds<Pixels>, b: &Bounds<Pixels>) -> bool {
    let threshold = 2.0;
    (a.origin.x.0 - b.origin.x.0).abs() > threshold
        || (a.origin.y.0 - b.origin.y.0).abs() > threshold
        || (a.size.width.0 - b.size.width.0).abs() > threshold
        || (a.size.height.0 - b.size.height.0).abs() > threshold
}

/// Extension trait that adds matched geometry to all elements.
pub trait MatchedGeometryExt: Sized {
    /// Participate in a matched geometry transition.
    ///
    /// Elements with the same `(namespace, id)` pair share geometry across frames.
    /// When this element appears at a different position or size than the previous
    /// frame's element with the same ID, it animates from the old bounds to the new.
    ///
    /// Works bidirectionally: both expanding and collapsing transitions animate.
    /// Use `.with_corner_radii()` on the returned element to set clip corner radii
    /// that match your element's visual border radius.
    fn matched_geometry(
        self,
        namespace: impl Into<ElementId>,
        id: impl Into<ElementId>,
        animation: Animation,
    ) -> MatchedGeometryElement<Self>;
}

impl<E: IntoElement + 'static> MatchedGeometryExt for E {
    fn matched_geometry(
        self,
        namespace: impl Into<ElementId>,
        id: impl Into<ElementId>,
        animation: Animation,
    ) -> MatchedGeometryElement<Self> {
        let namespace = namespace.into();
        let id = id.into();
        let element_id = make_element_id(&namespace, &id);
        MatchedGeometryElement {
            element_id,
            matched_id: MatchedGeometryId { namespace, id },
            element: Some(self),
            animation,
            corner_radii: Corners::default(),
        }
    }
}

impl<E: IntoElement + 'static> IntoElement for MatchedGeometryElement<E> {
    type Element = MatchedGeometryElement<E>;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: IntoElement + 'static> Element for MatchedGeometryElement<E> {
    type RequestLayoutState = (AnyElement, Option<LayoutId>);
    type PrepaintState = bool;

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let element = self.element.take().expect("should only be called once");
        let mut any_element = element.into_any_element();
        let child_layout_id = any_element.request_layout(window, cx);

        // When the previous frame was animating (natural_bounds is Some), create a
        // wrapper layout at the previous rendered size. This forces flexible children
        // (e.g. size_full()) to fill the larger space so the content mask can clip
        // them down during collapse, producing smooth size animation.
        let prev = window.previous_matched_geometry(&self.matched_id);
        if let Some(entry) = prev {
            if entry.natural_bounds.is_some() {
                let mut style = Style::default();
                style.size.width = entry.bounds.size.width.into();
                style.size.height = entry.bounds.size.height.into();
                let wrapper_id = window.request_layout(style, [child_layout_id], cx);
                return (wrapper_id, (any_element, Some(child_layout_id)));
            }
        }

        (child_layout_id, (any_element, None))
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (element, _child_layout_id): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let prev_entry = window.previous_matched_geometry(&self.matched_id).cloned();
        let prev_bounds = prev_entry.as_ref().map(|e| e.bounds);

        // When a wrapper was created, the natural target is stored from the previous
        // frame's entry. Otherwise the layout bounds ARE the natural bounds.
        let natural_bounds = prev_entry
            .as_ref()
            .and_then(|e| e.natural_bounds)
            .unwrap_or(bounds);

        if let Some(gid) = global_id {
            let duration = self.animation.duration;
            let easing = self.animation.easing.clone();
            let corner_radii = self.corner_radii.clone();

            let result = window.with_element_state(
                gid,
                |state: Option<Option<MatchedGeometryAnimState>>, _window| {
                    let state = state.flatten();

                    if let Some(anim) = state {
                        // Existing animation — continue it.
                        let elapsed = anim.start.elapsed().as_secs_f32();
                        let raw_delta = (elapsed / duration.as_secs_f32()).min(1.0);
                        let done = raw_delta >= 1.0;

                        if done {
                            // Check if a new animation is needed (e.g. direction reversed).
                            if let Some(prev) = prev_bounds {
                                if bounds_differ(&prev, &natural_bounds) {
                                    let new_anim = MatchedGeometryAnimState {
                                        start: Instant::now(),
                                        from_bounds: prev,
                                        to_bounds: natural_bounds,
                                    };
                                    (Some(prev), Some(new_anim))
                                } else {
                                    (None, None)
                                }
                            } else {
                                (None, None)
                            }
                        } else {
                            let eased_delta = (easing)(raw_delta);
                            let interpolated =
                                lerp_bounds(&anim.from_bounds, &anim.to_bounds, eased_delta);
                            (Some(interpolated), Some(anim))
                        }
                    } else if let Some(prev) = prev_bounds {
                        // No active animation — start one if bounds changed.
                        if bounds_differ(&prev, &natural_bounds) {
                            let anim = MatchedGeometryAnimState {
                                start: Instant::now(),
                                from_bounds: prev,
                                to_bounds: natural_bounds,
                            };
                            (Some(prev), Some(anim))
                        } else {
                            (None, None)
                        }
                    } else {
                        (None, None)
                    }
                },
            );

            if let Some(interpolated) = result {
                // Register interpolated bounds + the natural target for next frame.
                window.register_matched_geometry(
                    self.matched_id.clone(),
                    interpolated,
                    Some(natural_bounds),
                );

                // Position offset.
                let current_offset = window.element_offset();
                let offset = Point {
                    x: current_offset.x + (interpolated.origin.x - bounds.origin.x),
                    y: current_offset.y + (interpolated.origin.y - bounds.origin.y),
                };

                // Always clip to interpolated bounds for smooth size animation
                // in both expand and collapse directions.
                let content_mask = Some(ContentMask {
                    bounds: interpolated,
                    corner_radii: corner_radii,
                });

                window.defer_draw(
                    mem::replace(element, crate::Empty.into_any_element()),
                    offset,
                    usize::MAX,
                    content_mask,
                );
                window.request_animation_frame();
                return true;
            }
        }

        // Not animating: register natural bounds for future transitions.
        window.register_matched_geometry(self.matched_id.clone(), bounds, None);
        element.prepaint(window, cx);
        false
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        (element, _): &mut Self::RequestLayoutState,
        deferred: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !*deferred {
            element.paint(window, cx);
        }
    }
}
