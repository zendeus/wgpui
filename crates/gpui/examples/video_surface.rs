//! Example: animated video surface using synthetic NV12 test pattern.
//!
//! Demonstrates the `surface()` element rendering a `VideoFrame::Buffer`
//! with NV12 (YCbCr 4:2:0) format. Works on all platforms via CPU copy.
//!
//! Run with: `cargo run --example video_surface`

use gpui::{
    App, Bounds, Context, ObjectFit, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb,
    size, surface, VideoFrame, VideoFrameFormat,
};
use gpui_platform::application;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

struct VideoSurfaceExample {
    frame: VideoFrame,
    frame_count: u32,
}

impl VideoSurfaceExample {
    fn new() -> Self {
        let frame = generate_nv12_test_pattern(WIDTH, HEIGHT, 0);
        Self {
            frame,
            frame_count: 0,
        }
    }
}

impl Render for VideoSurfaceExample {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Request continuous animation
        window.request_animation_frame();

        // Advance frame
        self.frame_count = self.frame_count.wrapping_add(1);
        self.frame = generate_nv12_test_pattern(WIDTH, HEIGHT, self.frame_count);

        div()
            .size_full()
            .bg(rgb(0x1a1a2e))
            .flex()
            .flex_col()
            .justify_center()
            .items_center()
            .gap_4()
            .child(
                div()
                    .text_color(rgb(0xcccccc))
                    .text_sm()
                    .child(format!(
                        "Video Surface — {}x{} NV12 @ frame {}",
                        WIDTH, HEIGHT, self.frame_count
                    )),
            )
            .child(
                surface(self.frame.clone())
                    .object_fit(ObjectFit::Contain)
                    .w(px(WIDTH as f32))
                    .h(px(HEIGHT as f32)),
            )
    }
}

/// Generate a synthetic NV12 test pattern with animated elements.
///
/// Creates SMPTE-style color bars with a moving vertical line to
/// demonstrate that the surface updates each frame.
fn generate_nv12_test_pattern(width: u32, height: u32, frame: u32) -> VideoFrame {
    let y_stride = width;
    let cbcr_width = width / 2;
    let cbcr_height = height / 2;
    let cbcr_stride = cbcr_width * 2; // RG8 = 2 bytes per texel

    let mut y_plane = vec![0u8; (y_stride * height) as usize];
    let mut cbcr_plane = vec![128u8; (cbcr_stride * cbcr_height) as usize];

    // SMPTE color bars (Y, Cb, Cr values)
    // White, Yellow, Cyan, Green, Magenta, Red, Blue, Black
    let bars: [(u8, u8, u8); 8] = [
        (235, 128, 128), // White
        (210, 16, 146),  // Yellow
        (170, 166, 16),  // Cyan
        (145, 54, 34),   // Green
        (106, 202, 222), // Magenta
        (81, 90, 240),   // Red
        (41, 240, 110),  // Blue
        (16, 128, 128),  // Black
    ];

    let bar_width = width / 8;

    // Fill Y plane
    for row in 0..height {
        for col in 0..width {
            let bar_idx = ((col / bar_width) as usize).min(7);
            let (y, _, _) = bars[bar_idx];

            // Add a moving vertical bright line
            let line_x = ((frame * 3) % width) as i32;
            let dist = ((col as i32) - line_x).unsigned_abs();
            let brightness = if dist < 4 { 235u8 } else { y };

            // Add a horizontal gradient in the bottom quarter for visual interest
            let final_y = if row > height * 3 / 4 {
                let gradient = ((col as f32 / width as f32) * 219.0) as u8 + 16;
                gradient
            } else {
                brightness
            };

            y_plane[(row * y_stride + col) as usize] = final_y;
        }
    }

    // Fill CbCr plane (half resolution)
    for row in 0..cbcr_height {
        for col in 0..cbcr_width {
            let src_col = col * 2;
            let bar_idx = ((src_col / bar_width) as usize).min(7);
            let (_, cb, cr) = bars[bar_idx];

            // Bottom quarter: neutral chroma for gradient
            let (final_cb, final_cr) = if row > cbcr_height * 3 / 4 {
                (128, 128)
            } else {
                (cb, cr)
            };

            let offset = (row * cbcr_stride + col * 2) as usize;
            cbcr_plane[offset] = final_cb;
            cbcr_plane[offset + 1] = final_cr;
        }
    }

    VideoFrame::Buffer {
        planes: vec![y_plane, cbcr_plane],
        strides: vec![y_stride, cbcr_stride],
        width,
        height,
        format: VideoFrameFormat::Nv12,
    }
}

fn run_example() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.), px(500.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| VideoSurfaceExample::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    env_logger::init();
    run_example();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_example();
}
