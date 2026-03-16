#![cfg_attr(target_family = "wasm", no_main)]

use gpui::{
    App, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_platform::application;

struct CornerClipping;

impl Render for CornerClipping {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_6()
            .bg(rgb(0x1e1e2e))
            .size_full()
            .p_8()
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(0xcdd6f4))
                    .child("Rounded Corner Clipping Demo"),
            )
            .child(
                div()
                    .flex()
                    .gap_6()
                    // Test 1: Rounded container with overflowing colored children
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xa6adc8))
                                    .child("Overflow hidden + rounded corners"),
                            )
                            .child(
                                div()
                                    .w(px(200.))
                                    .h(px(200.))
                                    .rounded(px(24.))
                                    .overflow_hidden()
                                    .bg(rgb(0x313244))
                                    .border_1()
                                    .border_color(rgb(0x585b70))
                                    // Child that overflows into corners
                                    .child(
                                        div()
                                            .w(px(120.))
                                            .h(px(120.))
                                            .bg(rgb(0xf38ba8))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(rgb(0x1e1e2e))
                                                    .p_2()
                                                    .child("Top-left clipped"),
                                            ),
                                    )
                                    // Bottom-right child
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row_reverse()
                                            .child(
                                                div()
                                                    .w(px(120.))
                                                    .h(px(120.))
                                                    .bg(rgb(0xa6e3a1)),
                                            ),
                                    ),
                            ),
                    )
                    // Test 2: Without rounded corners (regression test)
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xa6adc8))
                                    .child("Overflow hidden, no rounding"),
                            )
                            .child(
                                div()
                                    .w(px(200.))
                                    .h(px(200.))
                                    .overflow_hidden()
                                    .bg(rgb(0x313244))
                                    .border_1()
                                    .border_color(rgb(0x585b70))
                                    .child(
                                        div()
                                            .w(px(120.))
                                            .h(px(120.))
                                            .bg(rgb(0x89b4fa)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row_reverse()
                                            .child(
                                                div()
                                                    .w(px(120.))
                                                    .h(px(120.))
                                                    .bg(rgb(0xfab387)),
                                            ),
                                    ),
                            ),
                    )
                    // Test 3: Large border radius (pill shape)
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xa6adc8))
                                    .child("Pill shape clipping"),
                            )
                            .child(
                                div()
                                    .w(px(200.))
                                    .h(px(80.))
                                    .rounded(px(40.))
                                    .overflow_hidden()
                                    .bg(rgb(0x313244))
                                    .border_1()
                                    .border_color(rgb(0x585b70))
                                    .flex()
                                    .child(
                                        div()
                                            .w(px(80.))
                                            .h(px(80.))
                                            .bg(rgb(0xcba6f7)),
                                    )
                                    .child(
                                        div()
                                            .w(px(80.))
                                            .h(px(80.))
                                            .bg(rgb(0xf9e2af)),
                                    )
                                    .child(
                                        div()
                                            .w(px(80.))
                                            .h(px(80.))
                                            .bg(rgb(0x94e2d5)),
                                    ),
                            ),
                    ),
            )
            // Test 4: Nested rounded containers
            .child(
                div()
                    .flex()
                    .gap_6()
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xa6adc8))
                                    .child("Nested rounded containers"),
                            )
                            .child(
                                div()
                                    .w(px(250.))
                                    .h(px(200.))
                                    .rounded(px(24.))
                                    .overflow_hidden()
                                    .bg(rgb(0x313244))
                                    .border_1()
                                    .border_color(rgb(0x585b70))
                                    .p_4()
                                    .child(
                                        div()
                                            .w_full()
                                            .h_full()
                                            .rounded(px(16.))
                                            .overflow_hidden()
                                            .bg(rgb(0x45475a))
                                            .p_2()
                                            .child(
                                                div()
                                                    .w(px(180.))
                                                    .h(px(180.))
                                                    .bg(rgb(0xf38ba8)),
                                            ),
                                    ),
                            ),
                    )
                    // Test 5: Text clipping at rounded corners
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0xa6adc8))
                                    .child("Text clipping at corners"),
                            )
                            .child(
                                div()
                                    .w(px(200.))
                                    .h(px(100.))
                                    .rounded(px(24.))
                                    .overflow_hidden()
                                    .bg(rgb(0x313244))
                                    .border_1()
                                    .border_color(rgb(0x585b70))
                                    .text_color(rgb(0xcdd6f4))
                                    .text_sm()
                                    .p_1()
                                    .child(
                                        "This text should be clipped at the rounded corners of the container. It extends to all edges to demonstrate the clipping effect.",
                                    ),
                            ),
                    ),
            )
    }
}

fn run_example() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.), px(550.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| CornerClipping),
        )
        .unwrap();
        cx.activate(true);
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
