#![cfg_attr(target_family = "wasm", no_main)]

use gpui::{
    Animation, App, Bounds, Context, ListAnimation, ListState, RemovedItem, Window, WindowBounds,
    WindowOptions, div, list, prelude::*, px, rgb, size, ListAlignment,
};
use gpui_platform::application;
use std::rc::Rc;
use std::time::Duration;

struct AnimatedVariableListExample {
    items: Vec<(String, f32)>, // (label, height)
    list_state: ListState,
    next_id: usize,
}

impl AnimatedVariableListExample {
    fn new() -> Self {
        let heights = [30.0, 50.0, 40.0, 60.0, 35.0, 55.0, 45.0, 70.0, 38.0, 48.0];
        let items: Vec<(String, f32)> = (1..=10)
            .map(|i| (format!("Item {i}"), heights[i - 1]))
            .collect();
        let list_state = ListState::new(items.len(), ListAlignment::Top, px(100.));
        list_state.animate(
            ListAnimation::new()
                .on_insert(
                    Animation::new(Duration::from_millis(300)).with_easing(gpui::ease_in_out),
                )
                .on_remove(
                    Animation::new(Duration::from_millis(250)).with_easing(gpui::ease_in_out),
                )
                .on_move(
                    Animation::new(Duration::from_millis(400)).with_easing(gpui::ease_in_out),
                ),
        );
        Self {
            next_id: items.len() + 1,
            items,
            list_state,
        }
    }

    fn insert_at_top(&mut self, _: &mut Window, _cx: &mut Context<Self>) {
        let height = 30.0 + (self.next_id as f32 % 5.0) * 10.0;
        let label = format!("Item {}", self.next_id);
        self.next_id += 1;
        self.items.insert(0, (label, height));
        self.list_state.splice(0..0, 1);
        self.list_state.notify_inserted(0..1);
    }

    fn insert_at_middle(&mut self, _: &mut Window, _cx: &mut Context<Self>) {
        let mid = self.items.len() / 2;
        let height = 30.0 + (self.next_id as f32 % 5.0) * 10.0;
        let label = format!("Item {}", self.next_id);
        self.next_id += 1;
        self.items.insert(mid, (label, height));
        self.list_state.splice(mid..mid, 1);
        self.list_state.notify_inserted(mid..mid + 1);
    }

    fn remove_first(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        let (label, height) = self.items[0].clone();
        self.list_state.notify_removed(vec![RemovedItem {
            index: 0,
            render: Rc::new(move |_window, _cx| {
                div()
                    .id("ghost-0")
                    .px_2()
                    .py_1()
                    .h(px(height))
                    .child(label.clone())
                    .into_any_element()
            }),
        }]);
        self.items.remove(0);
        self.list_state.splice(0..1, 0);
    }

    fn swap_first_two(&mut self, _: &mut Window, _cx: &mut Context<Self>) {
        if self.items.len() < 2 {
            return;
        }
        self.list_state.notify_moved(vec![(0, 1), (1, 0)]);
        self.items.swap(0, 1);
        self.list_state.splice(0..2, 2);
    }
}

impl Render for AnimatedVariableListExample {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.items.clone();

        div()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .flex()
            .flex_col()
            .child(
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
                list(
                    self.list_state.clone(),
                    move |ix, _window, _cx| {
                        let (label, height) = &items[ix];
                        let bg = if ix % 2 == 0 {
                            rgb(0x313244)
                        } else {
                            rgb(0x45475a)
                        };
                        div()
                            .px_2()
                            .py_1()
                            .h(px(*height))
                            .bg(bg)
                            .child(format!("{label} (h={height})"))
                            .into_any_element()
                    },
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
            |_, cx| cx.new(|_| AnimatedVariableListExample::new()),
        )
        .unwrap();
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_example();
}
