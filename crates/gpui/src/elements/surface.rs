use crate::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    ObjectFit, Pixels, Style, StyleRefinement, Styled, Window,
};
#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;
use refineable::Refineable;
#[cfg(target_os = "linux")]
use std::os::fd::OwnedFd;
#[cfg(target_os = "linux")]
use std::sync::Arc;

/// Pixel format of a video frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoFrameFormat {
    /// Biplanar YCbCr 4:2:0 (plane 0: Y, plane 1: CbCr interleaved).
    /// This is the default output format of hardware video decoders.
    Nv12,
    /// 32-bit BGRA, single plane.
    Bgra,
}

/// Per-plane descriptor for a DMA-BUF video frame.
#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
pub struct DmaBufPlane {
    /// Byte offset from the start of the DMA-BUF to this plane.
    pub offset: u32,
    /// Byte stride (row pitch) for this plane.
    pub stride: u32,
    /// DRM format modifier (e.g. `DRM_FORMAT_MOD_LINEAR`).
    pub modifier: u64,
}

/// A video frame from any source (LiveKit, hardware decoder, etc.)
///
/// # Platform-specific zero-copy variants
///
/// - **macOS**: `CoreVideo` wraps a `CVPixelBuffer` backed by an IOSurface.
///   The renderer imports it directly into Metal via wgpu HAL — no CPU copy.
/// - **Linux**: `DmaBuf` carries DMA-BUF file descriptors for Vulkan external
///   memory import.
/// - **All platforms**: `Buffer` carries raw pixel data and is uploaded via
///   `queue.write_texture()`.
#[derive(Clone, Debug)]
pub enum VideoFrame {
    /// A macOS CVPixelBuffer, typically IOSurface-backed for zero-copy GPU access.
    #[cfg(target_os = "macos")]
    CoreVideo(CVPixelBuffer),

    /// A Linux DMA-BUF for zero-copy Vulkan import.
    /// Falls back to mmap + CPU copy if Vulkan external memory extensions are unavailable.
    #[cfg(target_os = "linux")]
    DmaBuf {
        /// DMA-BUF file descriptor (shared via Arc for Clone).
        fd: Arc<OwnedFd>,
        /// Frame width in pixels.
        width: u32,
        /// Frame height in pixels.
        height: u32,
        /// Pixel format.
        format: VideoFrameFormat,
        /// Per-plane descriptors (offset, stride, modifier).
        planes: Vec<DmaBufPlane>,
    },

    /// CPU buffer fallback (works on all platforms).
    Buffer {
        /// Pixel data for each plane (e.g. Y and CbCr for NV12, or single plane for BGRA).
        planes: Vec<Vec<u8>>,
        /// Byte stride (row pitch) for each plane.
        strides: Vec<u32>,
        /// Frame width in pixels.
        width: u32,
        /// Frame height in pixels.
        height: u32,
        /// Pixel format.
        format: VideoFrameFormat,
    },
}

impl VideoFrame {
    /// Width of the video frame in pixels.
    pub fn width(&self) -> u32 {
        match self {
            #[cfg(target_os = "macos")]
            VideoFrame::CoreVideo(buf) => buf.get_width() as u32,
            #[cfg(target_os = "linux")]
            VideoFrame::DmaBuf { width, .. } => *width,
            VideoFrame::Buffer { width, .. } => *width,
        }
    }

    /// Height of the video frame in pixels.
    pub fn height(&self) -> u32 {
        match self {
            #[cfg(target_os = "macos")]
            VideoFrame::CoreVideo(buf) => buf.get_height() as u32,
            #[cfg(target_os = "linux")]
            VideoFrame::DmaBuf { height, .. } => *height,
            VideoFrame::Buffer { height, .. } => *height,
        }
    }

    /// Pixel format of the video frame.
    pub fn format(&self) -> VideoFrameFormat {
        match self {
            #[cfg(target_os = "macos")]
            VideoFrame::CoreVideo(_) => VideoFrameFormat::Nv12,
            #[cfg(target_os = "linux")]
            VideoFrame::DmaBuf { format, .. } => *format,
            VideoFrame::Buffer { format, .. } => *format,
        }
    }
}

#[cfg(target_os = "macos")]
impl From<CVPixelBuffer> for VideoFrame {
    fn from(value: CVPixelBuffer) -> Self {
        VideoFrame::CoreVideo(value)
    }
}

/// A surface element for displaying video frames.
pub struct Surface {
    source: VideoFrame,
    object_fit: ObjectFit,
    style: StyleRefinement,
}

/// Create a new surface element from a video frame.
pub fn surface(source: impl Into<VideoFrame>) -> Surface {
    Surface {
        source: source.into(),
        object_fit: ObjectFit::Contain,
        style: Default::default(),
    }
}

impl Surface {
    /// Set the object fit for the image.
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }
}

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
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
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let size = crate::size(
            crate::DevicePixels(self.source.width() as i32),
            crate::DevicePixels(self.source.height() as i32),
        );
        let new_bounds = self.object_fit.get_bounds(bounds, size);
        // TODO: Add support for corner_radii
        window.paint_surface(new_bounds, self.source.clone());
    }
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Surface {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
