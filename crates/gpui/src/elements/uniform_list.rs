//! A scrollable list of elements with uniform height, optimized for large lists.
//! Rather than use the full taffy layout system, uniform_list simply measures
//! the first element and then lays out all remaining elements in a line based on that
//! measurement. This is much faster than the full layout system, but only works for
//! elements with uniform height.

use crate::{
    AnyElement, Animation, App, AvailableSpace, Bounds, ContentMask, Corners, Element, ElementId,
    Entity, GlobalElementId, Hitbox, InspectorElementId, InteractiveElement, Interactivity,
    IntoElement, IsZero, LayoutId, ListSizingBehavior, Overflow, Pixels, Point, ScrollHandle, Size,
    StyleRefinement, Styled, Window, point, size,
};
use scheduler::Instant;
use smallvec::SmallVec;
use std::{cell::RefCell, cmp, ops::Range, rc::Rc, time::Duration, usize};

use super::ListHorizontalSizingBehavior;

/// Configuration for animating list item insertions, removals, and moves.
///
/// Attach to a `UniformList` via `.animate()`. Then call the notification methods
/// on [`UniformListScrollHandle`] when you mutate your data so the list knows
/// what changed and can drive the animations.
pub struct ListAnimation {
    /// Animation applied to newly inserted items (fade in).
    pub on_insert: Option<Animation>,
    /// Animation applied to removed items (fade out ghost).
    pub on_remove: Option<Animation>,
    /// Animation applied to moved items (slide to new position).
    pub on_move: Option<Animation>,
}

impl ListAnimation {
    /// Create a new, empty list animation configuration.
    pub fn new() -> Self {
        Self {
            on_insert: None,
            on_remove: None,
            on_move: None,
        }
    }

    /// Set the animation for newly inserted items.
    pub fn on_insert(mut self, animation: Animation) -> Self {
        self.on_insert = Some(animation);
        self
    }

    /// Set the animation for removed items (ghost fade-out).
    pub fn on_remove(mut self, animation: Animation) -> Self {
        self.on_remove = Some(animation);
        self
    }

    /// Set the animation for items that moved to a new index.
    pub fn on_move(mut self, animation: Animation) -> Self {
        self.on_move = Some(animation);
        self
    }
}

/// Describes an item that was removed from the list, for ghost animation.
/// Pass these to [`UniformListScrollHandle::notify_removed`] **before** you
/// mutate your backing data so the list can display ghost items during the
/// exit animation.
pub struct RemovedItem {
    /// The item's index in the list before removal.
    pub index: usize,
    /// A closure that renders the ghost element on each frame.
    /// Must produce a fresh element each call (elements are arena-allocated
    /// and cannot survive across frames).
    pub render: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>,
}

/// Queued in the scroll handle by notify_* calls, consumed during prepaint.
pub(crate) enum ListAnimationEvent {
    Inserted {
        range: Range<usize>,
    },
    Removed {
        removals: Vec<RemovedItem>,
    },
    Moved {
        /// Each tuple is (from_index, to_index).
        moves: Vec<(usize, usize)>,
    },
}

/// The kind of animation being applied to an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnimationKind {
    Insert,
    Remove,
    Move,
}

/// Per-item animation tracked across frames.
struct ActiveItemAnimation {
    kind: AnimationKind,
    start: Instant,
    duration: Duration,
    easing: std::rc::Rc<dyn Fn(f32) -> f32>,
    /// For Insert: the index of the inserted item.
    /// For Move: the old index before the move.
    from_index: usize,
    /// For Insert: same as from_index.
    /// For Move: the new index after the move.
    to_index: usize,
}

impl ActiveItemAnimation {
    /// Compute the eased animation delta [0.0, 1.0].
    /// Returns `None` if the animation has completed.
    fn delta(&self) -> Option<f32> {
        let elapsed = self.start.elapsed().as_secs_f32();
        let duration = self.duration.as_secs_f32();
        if duration <= 0.0 {
            return None;
        }
        let raw_delta = elapsed / duration;
        if raw_delta >= 1.0 {
            return None;
        }
        Some((self.easing)(raw_delta))
    }
}

/// A removed item still being rendered during its exit animation.
struct GhostItem {
    /// The item's index before removal (used for positioning).
    original_index: usize,
    start: Instant,
    duration: Duration,
    easing: std::rc::Rc<dyn Fn(f32) -> f32>,
    /// Closure to render the ghost element on each frame.
    render: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>,
}

impl GhostItem {
    /// Compute the eased animation delta [0.0, 1.0].
    /// Returns `None` if the animation has completed.
    fn delta(&self) -> Option<f32> {
        let elapsed = self.start.elapsed().as_secs_f32();
        let duration = self.duration.as_secs_f32();
        if duration <= 0.0 {
            return None;
        }
        let raw_delta = elapsed / duration;
        if raw_delta >= 1.0 {
            return None;
        }
        Some((self.easing)(raw_delta))
    }
}

/// Animation state persisted across frames in the scroll handle.
#[derive(Default)]
pub(crate) struct UniformListAnimationState {
    active: Vec<ActiveItemAnimation>,
    ghosts: Vec<GhostItem>,
}

/// uniform_list provides lazy rendering for a set of items that are of uniform height.
/// When rendered into a container with overflow-y: hidden and a fixed (or max) height,
/// uniform_list will only render the visible subset of items.
#[track_caller]
pub fn uniform_list<R>(
    id: impl Into<ElementId>,
    item_count: usize,
    f: impl 'static + Fn(Range<usize>, &mut Window, &mut App) -> Vec<R>,
) -> UniformList
where
    R: IntoElement,
{
    let id = id.into();
    let mut base_style = StyleRefinement::default();
    base_style.overflow.y = Some(Overflow::Scroll);

    let render_range = move |range: Range<usize>, window: &mut Window, cx: &mut App| {
        f(range, window, cx)
            .into_iter()
            .map(|component| component.into_any_element())
            .collect()
    };

    UniformList {
        item_count,
        item_to_measure_index: 0,
        render_items: Box::new(render_range),
        decorations: Vec::new(),
        interactivity: Interactivity {
            element_id: Some(id),
            base_style: Box::new(base_style),
            ..Interactivity::new()
        },
        scroll_handle: None,
        sizing_behavior: ListSizingBehavior::default(),
        horizontal_sizing_behavior: ListHorizontalSizingBehavior::default(),
        animation: None,
    }
}

/// A list element for efficiently laying out and displaying a list of uniform-height elements.
pub struct UniformList {
    item_count: usize,
    item_to_measure_index: usize,
    render_items: Box<
        dyn for<'a> Fn(Range<usize>, &'a mut Window, &'a mut App) -> SmallVec<[AnyElement; 64]>,
    >,
    decorations: Vec<Box<dyn UniformListDecoration>>,
    interactivity: Interactivity,
    scroll_handle: Option<UniformListScrollHandle>,
    sizing_behavior: ListSizingBehavior,
    horizontal_sizing_behavior: ListHorizontalSizingBehavior,
    animation: Option<ListAnimation>,
}

/// Frame state used by the [UniformList].
pub struct UniformListFrameState {
    items: SmallVec<[AnyElement; 32]>,
    item_opacities: SmallVec<[f32; 32]>,
    ghost_items: SmallVec<[(AnyElement, f32); 4]>,
    decorations: SmallVec<[AnyElement; 2]>,
}

/// A handle for controlling the scroll position of a uniform list.
/// This should be stored in your view and passed to the uniform_list on each frame.
#[derive(Clone, Debug, Default)]
pub struct UniformListScrollHandle(pub Rc<RefCell<UniformListScrollState>>);

/// Where to place the element scrolled to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollStrategy {
    /// Place the element at the top of the list's viewport.
    Top,
    /// Attempt to place the element in the middle of the list's viewport.
    /// May not be possible if there's not enough list items above the item scrolled to:
    /// in this case, the element will be placed at the closest possible position.
    Center,
    /// Attempt to place the element at the bottom of the list's viewport.
    /// May not be possible if there's not enough list items above the item scrolled to:
    /// in this case, the element will be placed at the closest possible position.
    Bottom,
    /// If the element is not visible attempt to place it at:
    /// - The top of the list's viewport if the target element is above currently visible elements.
    /// - The bottom of the list's viewport if the target element is above currently visible elements.
    Nearest,
}

#[derive(Clone, Copy, Debug)]
#[allow(missing_docs)]
pub struct DeferredScrollToItem {
    /// The item index to scroll to
    pub item_index: usize,
    /// The scroll strategy to use
    pub strategy: ScrollStrategy,
    /// The offset in number of items
    pub offset: usize,
    pub scroll_strict: bool,
}

#[allow(missing_docs)]
pub struct UniformListScrollState {
    pub base_handle: ScrollHandle,
    pub deferred_scroll_to_item: Option<DeferredScrollToItem>,
    /// Size of the item, captured during last layout.
    pub last_item_size: Option<ItemSize>,
    /// Whether the list was vertically flipped during last layout.
    pub y_flipped: bool,
    /// Pending animation events queued by notify_* calls, consumed during prepaint.
    pub(crate) pending_animation_events: Vec<ListAnimationEvent>,
    /// Active animation state, managed during prepaint.
    pub(crate) animation_state: UniformListAnimationState,
}

impl Default for UniformListScrollState {
    fn default() -> Self {
        Self {
            base_handle: ScrollHandle::default(),
            deferred_scroll_to_item: None,
            last_item_size: None,
            y_flipped: false,
            pending_animation_events: Vec::new(),
            animation_state: UniformListAnimationState::default(),
        }
    }
}

impl std::fmt::Debug for UniformListScrollState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UniformListScrollState")
            .field("base_handle", &self.base_handle)
            .field("deferred_scroll_to_item", &self.deferred_scroll_to_item)
            .field("last_item_size", &self.last_item_size)
            .field("y_flipped", &self.y_flipped)
            .field(
                "pending_animation_events",
                &self.pending_animation_events.len(),
            )
            .finish()
    }
}

#[derive(Copy, Clone, Debug, Default)]
/// The size of the item and its contents.
pub struct ItemSize {
    /// The size of the item.
    pub item: Size<Pixels>,
    /// The size of the item's contents, which may be larger than the item itself,
    /// if the item was bounded by a parent element.
    pub contents: Size<Pixels>,
}

impl UniformListScrollHandle {
    /// Create a new scroll handle to bind to a uniform list.
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(UniformListScrollState {
            base_handle: ScrollHandle::new(),
            deferred_scroll_to_item: None,
            last_item_size: None,
            y_flipped: false,
            pending_animation_events: Vec::new(),
            animation_state: UniformListAnimationState::default(),
        })))
    }

    /// Scroll the list so that the given item index is visible.
    ///
    /// This uses non-strict scrolling: if the item is already fully visible, no scrolling occurs.
    /// If the item is out of view, it scrolls the minimum amount to bring it into view according
    /// to the strategy.
    pub fn scroll_to_item(&self, ix: usize, strategy: ScrollStrategy) {
        self.0.borrow_mut().deferred_scroll_to_item = Some(DeferredScrollToItem {
            item_index: ix,
            strategy,
            offset: 0,
            scroll_strict: false,
        });
    }

    /// Scroll the list so that the given item index is at scroll strategy position.
    ///
    /// This uses strict scrolling: the item will always be scrolled to match the strategy position,
    /// even if it's already visible. Use this when you need precise positioning.
    pub fn scroll_to_item_strict(&self, ix: usize, strategy: ScrollStrategy) {
        self.0.borrow_mut().deferred_scroll_to_item = Some(DeferredScrollToItem {
            item_index: ix,
            strategy,
            offset: 0,
            scroll_strict: true,
        });
    }

    /// Scroll the list to the given item index with an offset in number of items.
    ///
    /// This uses non-strict scrolling: if the item is already visible within the offset region,
    /// no scrolling occurs.
    ///
    /// The offset parameter shrinks the effective viewport by the specified number of items
    /// from the corresponding edge, then applies the scroll strategy within that reduced viewport:
    /// - `ScrollStrategy::Top`: Shrinks from top, positions item at the new top
    /// - `ScrollStrategy::Center`: Shrinks from top, centers item in the reduced viewport
    /// - `ScrollStrategy::Bottom`: Shrinks from bottom, positions item at the new bottom
    pub fn scroll_to_item_with_offset(&self, ix: usize, strategy: ScrollStrategy, offset: usize) {
        self.0.borrow_mut().deferred_scroll_to_item = Some(DeferredScrollToItem {
            item_index: ix,
            strategy,
            offset,
            scroll_strict: false,
        });
    }

    /// Scroll the list so that the given item index is at the exact scroll strategy position with an offset.
    ///
    /// This uses strict scrolling: the item will always be scrolled to match the strategy position,
    /// even if it's already visible.
    ///
    /// The offset parameter shrinks the effective viewport by the specified number of items
    /// from the corresponding edge, then applies the scroll strategy within that reduced viewport:
    /// - `ScrollStrategy::Top`: Shrinks from top, positions item at the new top
    /// - `ScrollStrategy::Center`: Shrinks from top, centers item in the reduced viewport
    /// - `ScrollStrategy::Bottom`: Shrinks from bottom, positions item at the new bottom
    pub fn scroll_to_item_strict_with_offset(
        &self,
        ix: usize,
        strategy: ScrollStrategy,
        offset: usize,
    ) {
        self.0.borrow_mut().deferred_scroll_to_item = Some(DeferredScrollToItem {
            item_index: ix,
            strategy,
            offset,
            scroll_strict: true,
        });
    }

    /// Check if the list is flipped vertically.
    pub fn y_flipped(&self) -> bool {
        self.0.borrow().y_flipped
    }

    /// Get the index of the topmost visible child.
    #[cfg(any(test, feature = "test-support"))]
    pub fn logical_scroll_top_index(&self) -> usize {
        let this = self.0.borrow();
        this.deferred_scroll_to_item
            .as_ref()
            .map(|deferred| deferred.item_index)
            .unwrap_or_else(|| this.base_handle.logical_scroll_top().0)
    }

    /// Checks if the list can be scrolled vertically.
    pub fn is_scrollable(&self) -> bool {
        if let Some(size) = self.0.borrow().last_item_size {
            size.contents.height > size.item.height
        } else {
            false
        }
    }

    /// Scroll to the bottom of the list.
    pub fn scroll_to_bottom(&self) {
        self.scroll_to_item(usize::MAX, ScrollStrategy::Bottom);
    }

    /// Notify the list that items were inserted at the given range.
    /// Call this **after** you have inserted items into your backing data.
    pub fn notify_inserted(&self, range: Range<usize>) {
        self.0
            .borrow_mut()
            .pending_animation_events
            .push(ListAnimationEvent::Inserted { range });
    }

    /// Notify the list that items will be removed.
    /// Call this **before** you remove items from your backing data, providing
    /// pre-rendered elements for ghost display during the exit animation.
    pub fn notify_removed(&self, removals: Vec<RemovedItem>) {
        self.0
            .borrow_mut()
            .pending_animation_events
            .push(ListAnimationEvent::Removed { removals });
    }

    /// Notify the list that items were moved to new positions.
    /// Each tuple is `(from_index, to_index)`.
    /// Call this **after** you have reordered your backing data.
    pub fn notify_moved(&self, moves: Vec<(usize, usize)>) {
        self.0
            .borrow_mut()
            .pending_animation_events
            .push(ListAnimationEvent::Moved { moves });
    }
}

impl Styled for UniformList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl Element for UniformList {
    type RequestLayoutState = UniformListFrameState;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let max_items = self.item_count;
        let item_size = self.measure_item(None, window, cx);
        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| match self.sizing_behavior {
                ListSizingBehavior::Infer => {
                    window.with_text_style(style.text_style().cloned(), |window| {
                        window.request_measured_layout(
                            style,
                            move |known_dimensions, available_space, _window, _cx| {
                                let desired_height = item_size.height * max_items;
                                let width = known_dimensions.width.unwrap_or(match available_space
                                    .width
                                {
                                    AvailableSpace::Definite(x) => x,
                                    AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                                        item_size.width
                                    }
                                });
                                let height = match available_space.height {
                                    AvailableSpace::Definite(height) => desired_height.min(height),
                                    AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                                        desired_height
                                    }
                                };
                                size(width, height)
                            },
                        )
                    })
                }
                ListSizingBehavior::Auto => window
                    .with_text_style(style.text_style().cloned(), |window| {
                        window.request_layout(style, None, cx)
                    }),
            },
        );

        (
            layout_id,
            UniformListFrameState {
                items: SmallVec::new(),
                item_opacities: SmallVec::new(),
                ghost_items: SmallVec::new(),
                decorations: SmallVec::new(),
            },
        )
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        frame_state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Hitbox> {
        let style = self
            .interactivity
            .compute_style(global_id, None, window, cx);
        let border = style.border_widths.to_pixels(window.rem_size());
        let padding = style
            .padding
            .to_pixels(bounds.size.into(), window.rem_size());

        let padded_bounds = Bounds::from_corners(
            bounds.origin + point(border.left + padding.left, border.top + padding.top),
            bounds.bottom_right()
                - point(border.right + padding.right, border.bottom + padding.bottom),
        );

        let can_scroll_horizontally = matches!(
            self.horizontal_sizing_behavior,
            ListHorizontalSizingBehavior::Unconstrained
        );

        let longest_item_size = self.measure_item(None, window, cx);
        let content_width = if can_scroll_horizontally {
            padded_bounds.size.width.max(longest_item_size.width)
        } else {
            padded_bounds.size.width
        };
        let content_size = Size {
            width: content_width,
            height: longest_item_size.height * self.item_count,
        };

        let shared_scroll_offset = self.interactivity.scroll_offset.clone().unwrap();
        let item_height = longest_item_size.height;
        let shared_scroll_to_item = self.scroll_handle.as_mut().and_then(|handle| {
            let mut handle = handle.0.borrow_mut();
            handle.last_item_size = Some(ItemSize {
                item: padded_bounds.size,
                contents: content_size,
            });
            handle.deferred_scroll_to_item.take()
        });

        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            content_size,
            window,
            cx,
            |_style, mut scroll_offset, hitbox, window, cx| {
                let y_flipped = if let Some(scroll_handle) = &self.scroll_handle {
                    let scroll_state = scroll_handle.0.borrow();
                    scroll_state.y_flipped
                } else {
                    false
                };

                if self.item_count > 0 {
                    let content_height = item_height * self.item_count;

                    let is_scrolled_vertically = !scroll_offset.y.is_zero();
                    let max_scroll_offset = padded_bounds.size.height - content_height;

                    if is_scrolled_vertically && scroll_offset.y < max_scroll_offset {
                        shared_scroll_offset.borrow_mut().y = max_scroll_offset;
                        scroll_offset.y = max_scroll_offset;
                    }

                    let content_width = content_size.width + padding.left + padding.right;
                    let is_scrolled_horizontally =
                        can_scroll_horizontally && !scroll_offset.x.is_zero();
                    if is_scrolled_horizontally && content_width <= padded_bounds.size.width {
                        shared_scroll_offset.borrow_mut().x = Pixels::ZERO;
                        scroll_offset.x = Pixels::ZERO;
                    }

                    if let Some(DeferredScrollToItem {
                        mut item_index,
                        mut strategy,
                        offset,
                        scroll_strict,
                    }) = shared_scroll_to_item
                    {
                        let max_item_index = self.item_count.saturating_sub(1);
                        item_index = item_index.min(max_item_index);
                        if y_flipped {
                            item_index = self.item_count.saturating_sub(item_index + 1);
                        }
                        let list_height = padded_bounds.size.height;
                        let mut updated_scroll_offset = shared_scroll_offset.borrow_mut();
                        let item_top = item_height * item_index;
                        let item_bottom = item_top + item_height;
                        let scroll_top = -updated_scroll_offset.y;
                        let offset_pixels = item_height * offset;

                        // is the selected item above/below currently visible items
                        let is_above = item_top < scroll_top + offset_pixels;
                        let is_below = item_bottom > scroll_top + list_height;

                        if scroll_strict || is_above || is_below {
                            if strategy == ScrollStrategy::Nearest {
                                if is_above {
                                    strategy = ScrollStrategy::Top;
                                } else if is_below {
                                    strategy = ScrollStrategy::Bottom;
                                }
                            }

                            let max_scroll_offset =
                                (content_height - list_height).max(Pixels::ZERO);
                            match strategy {
                                ScrollStrategy::Top => {
                                    updated_scroll_offset.y = -(item_top - offset_pixels)
                                        .clamp(Pixels::ZERO, max_scroll_offset);
                                }
                                ScrollStrategy::Center => {
                                    let item_center = item_top + item_height / 2.0;

                                    let viewport_height = list_height - offset_pixels;
                                    let viewport_center = offset_pixels + viewport_height / 2.0;
                                    let target_scroll_top = item_center - viewport_center;
                                    updated_scroll_offset.y =
                                        -target_scroll_top.clamp(Pixels::ZERO, max_scroll_offset);
                                }
                                ScrollStrategy::Bottom => {
                                    updated_scroll_offset.y = -(item_bottom - list_height)
                                        .clamp(Pixels::ZERO, max_scroll_offset);
                                }
                                ScrollStrategy::Nearest => {
                                    // Nearest, but the item is visible -> no scroll is required
                                }
                            }
                        }
                        scroll_offset = *updated_scroll_offset
                    }

                    let first_visible_element_ix =
                        (-(scroll_offset.y + padding.top) / item_height).floor() as usize;
                    let last_visible_element_ix = ((-scroll_offset.y + padded_bounds.size.height)
                        / item_height)
                        .ceil() as usize;

                    let visible_range = first_visible_element_ix
                        ..cmp::min(last_visible_element_ix, self.item_count);

                    let items = if y_flipped {
                        let flipped_range = self.item_count.saturating_sub(visible_range.end)
                            ..self.item_count.saturating_sub(visible_range.start);
                        let mut items = (self.render_items)(flipped_range, window, cx);
                        items.reverse();
                        items
                    } else {
                        (self.render_items)(visible_range.clone(), window, cx)
                    };

                    // --- Animation state processing ---
                    let has_active_animations;

                    if let (Some(animation_config), Some(scroll_handle)) =
                        (&self.animation, &self.scroll_handle)
                    {
                        let mut handle = scroll_handle.0.borrow_mut();
                        let pending: Vec<ListAnimationEvent> =
                            handle.pending_animation_events.drain(..).collect();
                        let state = &mut handle.animation_state;

                        // Cleanup expired animations
                        state.active.retain(|a| a.delta().is_some());
                        state.ghosts.retain(|g| g.delta().is_some());

                        // Process new events
                        let now = Instant::now();
                        for event in pending {
                            match event {
                                ListAnimationEvent::Inserted { range } => {
                                    if let Some(ref anim) = animation_config.on_insert {
                                        for ix in range {
                                            state.active.push(ActiveItemAnimation {
                                                kind: AnimationKind::Insert,
                                                start: now,
                                                duration: anim.duration,
                                                easing: anim.easing.clone(),
                                                from_index: ix,
                                                to_index: ix,
                                            });
                                        }
                                    }
                                }
                                ListAnimationEvent::Removed { removals } => {
                                    if let Some(ref anim) = animation_config.on_remove {
                                        for removal in removals {
                                            state.ghosts.push(GhostItem {
                                                original_index: removal.index,
                                                start: now,
                                                duration: anim.duration,
                                                easing: anim.easing.clone(),
                                                render: removal.render,
                                            });
                                        }
                                    }
                                }
                                ListAnimationEvent::Moved { moves } => {
                                    if let Some(ref anim) = animation_config.on_move {
                                        for (from, to) in moves {
                                            state.active.push(ActiveItemAnimation {
                                                kind: AnimationKind::Move,
                                                start: now,
                                                duration: anim.duration,
                                                easing: anim.easing.clone(),
                                                from_index: from,
                                                to_index: to,
                                            });
                                        }
                                    }
                                }
                            }
                        }

                        has_active_animations =
                            !state.active.is_empty() || !state.ghosts.is_empty();
                    } else {
                        has_active_animations = false;
                    }

                    // Pre-compute per-item animation values and ghost metadata.
                    let mut item_anim_values: SmallVec<[(Pixels, f32); 32]> = SmallVec::new();
                    // (original_index, opacity, ghost_index_in_state)
                    let mut ghost_meta: SmallVec<[(usize, f32); 4]> = SmallVec::new();

                    if has_active_animations {
                        if let Some(scroll_handle) = &self.scroll_handle {
                            let handle = scroll_handle.0.borrow();
                            let state = &handle.animation_state;

                            // Compute per-item y-offset and opacity
                            for ix in visible_range.clone() {
                                item_anim_values.push(
                                    compute_item_animation(ix, state, item_height),
                                );
                            }

                            // Collect ghost metadata (index + opacity)
                            for ghost in &state.ghosts {
                                if let Some(delta) = ghost.delta() {
                                    ghost_meta.push((
                                        ghost.original_index,
                                        1.0 - delta,
                                    ));
                                }
                            }
                        }
                    }

                    // Collect ghost render closures (Rc-cloned) so we can call
                    // them without holding the scroll handle borrow.
                    let mut ghost_renders: SmallVec<
                        [(usize, f32, Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>); 4],
                    > = SmallVec::new();
                    if !ghost_meta.is_empty() {
                        if let Some(scroll_handle) = &self.scroll_handle {
                            let handle = scroll_handle.0.borrow();
                            for (i, &(original_index, ghost_opacity)) in
                                ghost_meta.iter().enumerate()
                            {
                                if let Some(ghost) = handle.animation_state.ghosts.get(i) {
                                    ghost_renders.push((
                                        original_index,
                                        ghost_opacity,
                                        ghost.render.clone(),
                                    ));
                                }
                            }
                        }
                    }

                    // Render ghost elements (outside the borrow).
                    let mut ghost_prepaint_data: SmallVec<[(usize, f32, AnyElement); 4]> =
                        SmallVec::new();
                    for (original_index, ghost_opacity, render) in ghost_renders {
                        let element = render(window, cx);
                        ghost_prepaint_data.push((original_index, ghost_opacity, element));
                    }

                    let content_mask = ContentMask { bounds, corner_radii: Corners::default() };
                    window.with_content_mask(Some(content_mask), |window| {
                        for (i, (mut item, ix)) in
                            items.into_iter().zip(visible_range.clone()).enumerate()
                        {
                            let (anim_y_offset, anim_opacity) = item_anim_values
                                .get(i)
                                .copied()
                                .unwrap_or((Pixels::ZERO, 1.0));

                            let item_origin = padded_bounds.origin
                                + scroll_offset
                                + point(Pixels::ZERO, item_height * ix + anim_y_offset);

                            let available_width = if can_scroll_horizontally {
                                padded_bounds.size.width + scroll_offset.x.abs()
                            } else {
                                padded_bounds.size.width
                            };
                            let available_space = size(
                                AvailableSpace::Definite(available_width),
                                AvailableSpace::Definite(item_height),
                            );
                            item.layout_as_root(available_space, window, cx);
                            item.prepaint_at(item_origin, window, cx);
                            frame_state.items.push(item);
                            frame_state.item_opacities.push(anim_opacity);
                        }

                        // Prepaint ghost items (removed items still animating out)
                        for (original_index, ghost_opacity, mut element) in ghost_prepaint_data {
                            let ghost_origin = padded_bounds.origin
                                + scroll_offset
                                + point(Pixels::ZERO, item_height * original_index);

                            let available_width = if can_scroll_horizontally {
                                padded_bounds.size.width + scroll_offset.x.abs()
                            } else {
                                padded_bounds.size.width
                            };
                            let available_space = size(
                                AvailableSpace::Definite(available_width),
                                AvailableSpace::Definite(item_height),
                            );
                            element.layout_as_root(available_space, window, cx);
                            element.prepaint_at(ghost_origin, window, cx);
                            frame_state.ghost_items.push((element, ghost_opacity));
                        }

                        let bounds =
                            Bounds::new(padded_bounds.origin + scroll_offset, padded_bounds.size);
                        for decoration in &self.decorations {
                            let mut decoration = decoration.as_ref().compute(
                                visible_range.clone(),
                                bounds,
                                scroll_offset,
                                item_height,
                                self.item_count,
                                window,
                                cx,
                            );
                            let available_space = size(
                                AvailableSpace::Definite(bounds.size.width),
                                AvailableSpace::Definite(bounds.size.height),
                            );
                            decoration.layout_as_root(available_space, window, cx);
                            decoration.prepaint_at(bounds.origin, window, cx);
                            frame_state.decorations.push(decoration);
                        }
                    });

                    // Request next animation frame if animations are still active
                    if has_active_animations {
                        window.request_animation_frame();
                    }
                }

                hitbox
            },
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<crate::Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Option<Hitbox>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            |_, window, cx| {
                let has_opacities = !request_layout.item_opacities.is_empty();
                for (i, item) in request_layout.items.iter_mut().enumerate() {
                    let opacity = if has_opacities {
                        request_layout.item_opacities.get(i).copied()
                    } else {
                        None
                    };
                    // Only wrap with opacity if it's not fully opaque
                    if opacity.map_or(false, |o| o < 1.0) {
                        window.with_element_opacity(opacity, |window| {
                            item.paint(window, cx);
                        });
                    } else {
                        item.paint(window, cx);
                    }
                }
                // Paint ghost items with their opacity
                for (ghost, ghost_opacity) in &mut request_layout.ghost_items {
                    window.with_element_opacity(Some(*ghost_opacity), |window| {
                        ghost.paint(window, cx);
                    });
                }
                for decoration in &mut request_layout.decorations {
                    decoration.paint(window, cx);
                }
            },
        )
    }
}

/// Compute the y-offset and opacity for a given item index based on active animations.
fn compute_item_animation(
    ix: usize,
    state: &UniformListAnimationState,
    item_height: Pixels,
) -> (Pixels, f32) {
    let mut y_offset = Pixels::ZERO;
    let mut opacity = 1.0_f32;

    for anim in &state.active {
        match anim.kind {
            AnimationKind::Insert => {
                if let Some(delta) = anim.delta() {
                    if ix == anim.from_index {
                        // The inserted item itself: fade in
                        opacity = opacity.min(delta);
                    }
                    // Items at or after the insertion point slide down
                    // (they were displaced by the insertion)
                    // Not applied here — displacement is visual only if
                    // we tracked how many items were inserted in a batch.
                    // For now, just fade in the inserted item.
                }
            }
            AnimationKind::Move => {
                if let Some(delta) = anim.delta() {
                    if ix == anim.to_index {
                        // This item moved from from_index to to_index.
                        // Animate from old position to new position.
                        let from_y = item_height * anim.from_index;
                        let to_y = item_height * anim.to_index;
                        let offset = (from_y - to_y) * (1.0 - delta);
                        y_offset = y_offset + offset;
                    }
                }
            }
            AnimationKind::Remove => {
                // Removals are handled via ghost items, not active animations
            }
        }
    }

    (y_offset, opacity)
}

impl IntoElement for UniformList {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// A decoration for a [`UniformList`]. This can be used for various things,
/// such as rendering indent guides, or other visual effects.
pub trait UniformListDecoration {
    /// Compute the decoration element, given the visible range of list items,
    /// the bounds of the list, and the height of each item.
    fn compute(
        &self,
        visible_range: Range<usize>,
        bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        item_height: Pixels,
        item_count: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
}

impl<T: UniformListDecoration + 'static> UniformListDecoration for Entity<T> {
    fn compute(
        &self,
        visible_range: Range<usize>,
        bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        item_height: Pixels,
        item_count: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.update(cx, |inner, cx| {
            inner.compute(
                visible_range,
                bounds,
                scroll_offset,
                item_height,
                item_count,
                window,
                cx,
            )
        })
    }
}

impl UniformList {
    /// Selects a specific list item for measurement.
    pub fn with_width_from_item(mut self, item_index: Option<usize>) -> Self {
        self.item_to_measure_index = item_index.unwrap_or(0);
        self
    }

    /// Sets the sizing behavior, similar to the `List` element.
    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing_behavior = behavior;
        self
    }

    /// Sets the horizontal sizing behavior, controlling the way list items laid out horizontally.
    /// With [`ListHorizontalSizingBehavior::Unconstrained`] behavior, every item and the list itself will
    /// have the size of the widest item and lay out pushing the `end_slot` to the right end.
    pub fn with_horizontal_sizing_behavior(
        mut self,
        behavior: ListHorizontalSizingBehavior,
    ) -> Self {
        self.horizontal_sizing_behavior = behavior;
        match behavior {
            ListHorizontalSizingBehavior::FitList => {
                self.interactivity.base_style.overflow.x = None;
            }
            ListHorizontalSizingBehavior::Unconstrained => {
                self.interactivity.base_style.overflow.x = Some(Overflow::Scroll);
            }
        }
        self
    }

    /// Adds a decoration element to the list.
    pub fn with_decoration(mut self, decoration: impl UniformListDecoration + 'static) -> Self {
        self.decorations.push(Box::new(decoration));
        self
    }

    fn measure_item(
        &self,
        list_width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Size<Pixels> {
        if self.item_count == 0 {
            return Size::default();
        }

        let item_ix = cmp::min(self.item_to_measure_index, self.item_count - 1);
        let mut items = (self.render_items)(item_ix..item_ix + 1, window, cx);
        let Some(mut item_to_measure) = items.pop() else {
            return Size::default();
        };
        let available_space = size(
            list_width.map_or(AvailableSpace::MinContent, |width| {
                AvailableSpace::Definite(width)
            }),
            AvailableSpace::MinContent,
        );
        item_to_measure.layout_as_root(available_space, window, cx)
    }

    /// Enable animations for list item insertions, removals, and/or moves.
    ///
    /// When set, the list will animate items based on notifications sent via
    /// the [`UniformListScrollHandle`] methods `notify_inserted`, `notify_removed`,
    /// and `notify_moved`.
    pub fn animate(mut self, animation: ListAnimation) -> Self {
        self.animation = Some(animation);
        self
    }

    /// Track and render scroll state of this list with reference to the given scroll handle.
    pub fn track_scroll(mut self, handle: &UniformListScrollHandle) -> Self {
        self.interactivity.tracked_scroll_handle = Some(handle.0.borrow().base_handle.clone());
        self.scroll_handle = Some(handle.clone());
        self
    }

    /// Sets whether the list is flipped vertically, such that item 0 appears at the bottom.
    pub fn y_flipped(mut self, y_flipped: bool) -> Self {
        if let Some(ref scroll_handle) = self.scroll_handle {
            let mut scroll_state = scroll_handle.0.borrow_mut();
            let mut base_handle = &scroll_state.base_handle;
            let offset = base_handle.offset();
            match scroll_state.last_item_size {
                Some(last_size) if scroll_state.y_flipped != y_flipped => {
                    let new_y_offset =
                        -(offset.y + last_size.contents.height - last_size.item.height);
                    base_handle.set_offset(point(offset.x, new_y_offset));
                    scroll_state.y_flipped = y_flipped;
                }
                // Handle case where list is initially flipped.
                None if y_flipped => {
                    base_handle.set_offset(point(offset.x, Pixels::MIN));
                    scroll_state.y_flipped = y_flipped;
                }
                _ => {}
            }
        }
        self
    }
}

impl InteractiveElement for UniformList {
    fn interactivity(&mut self) -> &mut crate::Interactivity {
        &mut self.interactivity
    }
}

#[cfg(test)]
mod test {
    use crate::TestAppContext;

    #[gpui::test]
    fn test_scroll_strategy_nearest(cx: &mut TestAppContext) {
        use crate::{
            Context, FocusHandle, ScrollStrategy, UniformListScrollHandle, Window, div, prelude::*,
            px, uniform_list,
        };
        use std::ops::Range;

        actions!(example, [SelectNext, SelectPrev]);

        struct TestView {
            index: usize,
            length: usize,
            scroll_handle: UniformListScrollHandle,
            focus_handle: FocusHandle,
            visible_range: Range<usize>,
        }

        impl TestView {
            pub fn select_next(
                &mut self,
                _: &SelectNext,
                window: &mut Window,
                _: &mut Context<Self>,
            ) {
                if self.index + 1 == self.length {
                    self.index = 0
                } else {
                    self.index += 1;
                }
                self.scroll_handle
                    .scroll_to_item(self.index, ScrollStrategy::Nearest);
                window.refresh();
            }

            pub fn select_previous(
                &mut self,
                _: &SelectPrev,
                window: &mut Window,
                _: &mut Context<Self>,
            ) {
                if self.index == 0 {
                    self.index = self.length - 1
                } else {
                    self.index -= 1;
                }
                self.scroll_handle
                    .scroll_to_item(self.index, ScrollStrategy::Nearest);
                window.refresh();
            }
        }

        impl Render for TestView {
            fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .id("list-example")
                    .track_focus(&self.focus_handle)
                    .on_action(cx.listener(Self::select_next))
                    .on_action(cx.listener(Self::select_previous))
                    .size_full()
                    .child(
                        uniform_list(
                            "entries",
                            self.length,
                            cx.processor(|this, range: Range<usize>, _window, _cx| {
                                this.visible_range = range.clone();
                                range
                                    .map(|ix| div().id(ix).h(px(20.0)).child(format!("Item {ix}")))
                                    .collect()
                            }),
                        )
                        .track_scroll(&self.scroll_handle)
                        .h(px(200.0)),
                    )
            }
        }

        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle, cx);
            TestView {
                scroll_handle: UniformListScrollHandle::new(),
                index: 0,
                focus_handle,
                length: 47,
                visible_range: 0..0,
            }
        });

        // 10 out of 47 items are visible

        // First 9 times selecting next item does not scroll
        for ix in 1..10 {
            cx.dispatch_action(SelectNext);
            view.read_with(cx, |view, _| {
                assert_eq!(view.index, ix);
                assert_eq!(view.visible_range, 0..10);
            })
        }

        // Now each time the list scrolls down by 1
        for ix in 10..47 {
            cx.dispatch_action(SelectNext);
            view.read_with(cx, |view, _| {
                assert_eq!(view.index, ix);
                assert_eq!(view.visible_range, ix - 9..ix + 1);
            })
        }

        // After the last item we move back to the start
        cx.dispatch_action(SelectNext);
        view.read_with(cx, |view, _| {
            assert_eq!(view.index, 0);
            assert_eq!(view.visible_range, 0..10);
        });

        // Return to the last element
        cx.dispatch_action(SelectPrev);
        view.read_with(cx, |view, _| {
            assert_eq!(view.index, 46);
            assert_eq!(view.visible_range, 37..47);
        });

        // First 9 times selecting previous does not scroll
        for ix in (37..46).rev() {
            cx.dispatch_action(SelectPrev);
            view.read_with(cx, |view, _| {
                assert_eq!(view.index, ix);
                assert_eq!(view.visible_range, 37..47);
            })
        }

        // Now each time the list scrolls up by 1
        for ix in (0..37).rev() {
            cx.dispatch_action(SelectPrev);
            view.read_with(cx, |view, _| {
                assert_eq!(view.index, ix);
                assert_eq!(view.visible_range, ix..ix + 10);
            })
        }
    }
}
