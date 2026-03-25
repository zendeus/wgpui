#![cfg_attr(target_family = "wasm", no_main)]

use gpui::{
    Animation, App, Bounds, Context, ListAnimation, RemovedItem, UniformListScrollHandle, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size, uniform_list,
};
use gpui_platform::application;
use std::rc::Rc;
use std::time::Duration;

struct AnimatedListExample {
    items: Vec<String>,
    scroll_handle: UniformListScrollHandle,
    next_id: usize,
}

impl AnimatedListExample {
    fn new() -> Self {
        let items: Vec<String> = (1..=10).map(|i| format!("Item {i}")).collect();
        Self {
            next_id: items.len() + 1,
            items,
            scroll_handle: UniformListScrollHandle::new(),
        }
    }

    fn insert_at_top(&mut self, _: &mut Window, _cx: &mut Context<Self>) {
        let label = format!("Item {}", self.next_id);
        self.next_id += 1;
        self.items.insert(0, label);
        self.scroll_handle.notify_inserted(0..1);
    }

    fn insert_at_middle(&mut self, _: &mut Window, _cx: &mut Context<Self>) {
        let mid = self.items.len() / 2;
        let label = format!("Item {}", self.next_id);
        self.next_id += 1;
        self.items.insert(mid, label);
        self.scroll_handle.notify_inserted(mid..mid + 1);
    }

    fn remove_first(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        // Capture the label so the ghost render closure can produce it each frame
        let label = self.items[0].clone();
        self.scroll_handle.notify_removed(vec![RemovedItem {
            index: 0,
            render: Rc::new(move |_window, _cx| {
                div()
                    .id("ghost-0")
                    .px_2()
                    .py_1()
                    .child(label.clone())
                    .into_any_element()
            }),
        }]);
        self.items.remove(0);
    }

    fn swap_first_two(&mut self, _: &mut Window, _cx: &mut Context<Self>) {
        if self.items.len() < 2 {
            return;
        }
        self.items.swap(0, 1);
        self.scroll_handle.notify_moved(vec![(0, 1), (1, 0)]);
    }
}

impl Render for AnimatedListExample {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let item_count = self.items.len();

        div()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .flex()
            .flex_col()
            .child(
                // Button bar
                div()
                    .flex()
                    .gap_2()
                    .p_2()
                    .child(
                        div()
                            .id("btn-insert-top")
                            .px_3()
                            .py_1()
                            .bg(rgb(0x89b4fa))
                            .text_color(rgb(0x1e1e2e))
                            .rounded_md()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.insert_at_top(window, cx);
                            }))
                            .child("Insert Top"),
                    )
                    .child(
                        div()
                            .id("btn-insert-mid")
                            .px_3()
                            .py_1()
                            .bg(rgb(0xa6e3a1))
                            .text_color(rgb(0x1e1e2e))
                            .rounded_md()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.insert_at_middle(window, cx);
                            }))
                            .child("Insert Middle"),
                    )
                    .child(
                        div()
                            .id("btn-remove")
                            .px_3()
                            .py_1()
                            .bg(rgb(0xf38ba8))
                            .text_color(rgb(0x1e1e2e))
                            .rounded_md()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.remove_first(window, cx);
                            }))
                            .child("Remove First"),
                    )
                    .child(
                        div()
                            .id("btn-swap")
                            .px_3()
                            .py_1()
                            .bg(rgb(0xfab387))
                            .text_color(rgb(0x1e1e2e))
                            .rounded_md()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.swap_first_two(window, cx);
                            }))
                            .child("Swap 1st & 2nd"),
                    ),
            )
            .child(
                uniform_list(
                    "animated-entries",
                    item_count,
                    cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
                        range
                            .map(|ix| {
                                let bg = if ix % 2 == 0 {
                                    rgb(0x313244)
                                } else {
                                    rgb(0x45475a)
                                };
                                div()
                                    .id(ix)
                                    .px_2()
                                    .py_1()
                                    .bg(bg)
                                    .child(this.items[ix].clone())
                            })
                            .collect()
                    }),
                )
                .track_scroll(&self.scroll_handle)
                .animate(
                    ListAnimation::new()
                        .on_insert(
                            Animation::new(Duration::from_millis(300))
                                .with_easing(gpui::ease_in_out),
                        )
                        .on_remove(
                            Animation::new(Duration::from_millis(250))
                                .with_easing(gpui::ease_in_out),
                        )
                        .on_move(
                            Animation::new(Duration::from_millis(400))
                                .with_easing(gpui::ease_in_out),
                        ),
                )
                .flex_1(),
            )
    }
}

fn run_example() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(400.0), px(500.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| AnimatedListExample::new()),
        )
        .unwrap();
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_example();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_example();
}
