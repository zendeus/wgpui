#![cfg_attr(target_family = "wasm", no_main)]

use std::time::Duration;

use gpui::{
    Animation, App, Bounds, Context, MatchedGeometryExt as _, Window, WindowBounds, WindowOptions,
    div, ease_in_out, prelude::*, px, rgb, size, white,
};
use gpui_platform::application;

fn card_color(ix: usize) -> gpui::Hsla {
    let colors = [
        rgb(0xe74c3c), // red
        rgb(0x3498db), // blue
        rgb(0x2ecc71), // green
        rgb(0xf39c12), // orange
        rgb(0x9b59b6), // purple
        rgb(0x1abc9c), // teal
    ];
    colors[ix % colors.len()].into()
}

fn transition() -> Animation {
    Animation::new(Duration::from_millis(300)).with_easing(ease_in_out)
}

fn card_radius() -> gpui::Pixels {
    px(12.0)
}

struct MatchedGeometryExample {
    selected: Option<usize>,
}

impl Render for MatchedGeometryExample {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected;

        div()
            .id("root")
            .size_full()
            .bg(rgb(0x111122))
            .child(if let Some(card_ix) = selected {
                // Expanded: card centered with margin on all sides
                div()
                    .size_full()
                    .flex()
                    .justify_center()
                    .items_center()
                    .p(px(40.))
                    .child(
                        div()
                            .id(("card", card_ix))
                            .size_full()
                            .bg(card_color(card_ix))
                            .rounded_xl()
                            .p_6()
                            .flex()
                            .flex_col()
                            .justify_between()
                            .child(
                                div()
                                    .text_xl()
                                    .text_color(white())
                                    .child(format!("Card {} — Expanded", card_ix)),
                            )
                            .child(
                                div()
                                    .text_color(gpui::Hsla::white().opacity(0.6))
                                    .child("Click to collapse"),
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _window, _cx| {
                                this.selected = None;
                            }))
                            .matched_geometry("cards", card_ix, transition())
                            .with_corner_radii(card_radius()),
                    )
                    .into_any_element()
            } else {
                // Sidebar thumbnails on the left, empty space on the right
                div()
                    .size_full()
                    .flex()
                    .child(
                        // Left sidebar with card thumbnails
                        div()
                            .w(px(180.))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_3()
                            .children((0..6).map(|ix| {
                                // Outer container constrains the card to 70px in
                                // the sidebar. The card uses size_full() so it can
                                // fill the animation wrapper during collapse.
                                div().h(px(70.)).child(
                                    div()
                                        .id(("card", ix))
                                        .size_full()
                                        .bg(card_color(ix))
                                        .rounded_lg()
                                        .p_3()
                                        .flex()
                                        .items_center()
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(white())
                                                .child(format!("Card {}", ix)),
                                        )
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _window, _cx| {
                                            this.selected = Some(ix);
                                        }))
                                        .matched_geometry("cards", ix, transition())
                                        .with_corner_radii(card_radius()),
                                )
                            })),
                    )
                    .child(
                        // Right panel placeholder
                        div()
                            .flex_grow()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_color(gpui::Hsla::white().opacity(0.3))
                                    .child("Select a card"),
                            ),
                    )
                    .into_any_element()
            })
    }
}

fn run_example() {
    application().run(|cx: &mut App| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(600.), px(550.)),
                cx,
            ))),
            ..Default::default()
        };
        cx.open_window(options, |_, cx| {
            cx.activate(false);
            cx.new(|_| MatchedGeometryExample { selected: None })
        })
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
