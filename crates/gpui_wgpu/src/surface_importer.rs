use gpui::{VideoFrame, VideoFrameFormat};

/// A video frame imported into wgpu textures, ready for rendering.
pub struct ImportedSurface {
    /// Texture view for the Y plane (R8Unorm for NV12, Rgba8Unorm for BGRA).
    pub y_view: wgpu::TextureView,
    /// Texture view for the CbCr plane (Rg8Unorm for NV12).
    /// For BGRA format, this is a 1x1 dummy texture.
    pub cbcr_view: wgpu::TextureView,
    /// The format of the source frame.
    pub format: VideoFrameFormat,
    /// Keep textures alive for the duration of the frame.
    _textures: Vec<wgpu::Texture>,
    /// Keep CVMetalTextures alive (their lifetime backs the Metal textures).
    #[cfg(target_os = "macos")]
    _cv_metal_textures: Vec<core_video::metal_texture::CVMetalTexture>,
}

/// Import a `VideoFrame` into wgpu textures for rendering.
///
/// On macOS with `CoreVideo` frames, uses `CVMetalTextureCache` for zero-copy
/// GPU texture import. Falls back to CPU copy via `queue.write_texture()` for
/// `Buffer` frames on all platforms.
pub fn import_video_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &VideoFrame,
    #[cfg(target_os = "macos")] metal_texture_cache: Option<&core_video::metal_texture_cache::CVMetalTextureCache>,
) -> ImportedSurface {
    match frame {
        #[cfg(target_os = "macos")]
        VideoFrame::CoreVideo(cv_buf) => {
            if let Some(cache) = metal_texture_cache {
                import_core_video_zero_copy(device, cv_buf, cache)
            } else {
                import_core_video_cpu(device, queue, cv_buf)
            }
        }
        #[cfg(target_os = "linux")]
        VideoFrame::DmaBuf {
            fd,
            width,
            height,
            format,
            planes,
        } => import_dmabuf_cpu(device, queue, fd, planes, *width, *height, *format),
        VideoFrame::Buffer {
            planes,
            strides,
            width,
            height,
            format,
        } => import_buffer(device, queue, planes, strides, *width, *height, *format),
    }
}

/// Zero-copy import of a CVPixelBuffer via CVMetalTextureCache.
///
/// This creates Metal textures that directly reference the IOSurface backing
/// the CVPixelBuffer — no CPU→GPU copy occurs. The Metal textures are then
/// wrapped into wgpu textures via the HAL.
#[cfg(target_os = "macos")]
fn import_core_video_zero_copy(
    device: &wgpu::Device,
    cv_buf: &core_video::pixel_buffer::CVPixelBuffer,
    texture_cache: &core_video::metal_texture_cache::CVMetalTextureCache,
) -> ImportedSurface {
    use core_foundation::base::TCFType;
    use core_video::image_buffer::CVImageBufferRef;

    let width = cv_buf.get_width();
    let height = cv_buf.get_height();
    let plane_count = cv_buf.get_plane_count();
    let source_image = cv_buf.as_concrete_TypeRef() as CVImageBufferRef;

    if plane_count >= 2 {
        // NV12 biplanar: plane 0 = Y (R8Unorm), plane 1 = CbCr (RG8Unorm)
        let y_width = cv_buf.get_width_of_plane(0);
        let y_height = cv_buf.get_height_of_plane(0);
        let cbcr_width = cv_buf.get_width_of_plane(1);
        let cbcr_height = cv_buf.get_height_of_plane(1);

        let cv_y = texture_cache
            .create_texture_from_image(
                source_image,
                None,
                metal::MTLPixelFormat::R8Unorm,
                y_width,
                y_height,
                0,
            )
            .expect("Failed to create Y metal texture from CVPixelBuffer");

        let cv_cbcr = texture_cache
            .create_texture_from_image(
                source_image,
                None,
                metal::MTLPixelFormat::RG8Unorm,
                cbcr_width,
                cbcr_height,
                1,
            )
            .expect("Failed to create CbCr metal texture from CVPixelBuffer");

        let mtl_y = cv_y.get_texture().expect("CVMetalTexture has no texture");
        let mtl_cbcr = cv_cbcr
            .get_texture()
            .expect("CVMetalTexture has no texture");

        let (y_tex, cbcr_tex) = wrap_metal_textures_as_wgpu(
            device,
            &mtl_y,
            wgpu::TextureFormat::R8Unorm,
            y_width as u32,
            y_height as u32,
            &mtl_cbcr,
            wgpu::TextureFormat::Rg8Unorm,
            cbcr_width as u32,
            cbcr_height as u32,
        );

        let y_view = y_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let cbcr_view = cbcr_tex.create_view(&wgpu::TextureViewDescriptor::default());

        ImportedSurface {
            y_view,
            cbcr_view,
            format: VideoFrameFormat::Nv12,
            _textures: vec![y_tex, cbcr_tex],
            _cv_metal_textures: vec![cv_y, cv_cbcr],
        }
    } else {
        // Single plane BGRA
        let cv_tex = texture_cache
            .create_texture_from_image(
                source_image,
                None,
                metal::MTLPixelFormat::BGRA8Unorm,
                width,
                height,
                0,
            )
            .expect("Failed to create BGRA metal texture from CVPixelBuffer");

        let mtl_tex = cv_tex.get_texture().expect("CVMetalTexture has no texture");

        let wgpu_tex = wrap_single_metal_texture_as_wgpu(
            device,
            &mtl_tex,
            wgpu::TextureFormat::Bgra8Unorm,
            width as u32,
            height as u32,
        );

        let y_view = wgpu_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Dummy CbCr for bind group compatibility
        let dummy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("surface_dummy_cbcr"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let cbcr_view = dummy.create_view(&wgpu::TextureViewDescriptor::default());

        ImportedSurface {
            y_view,
            cbcr_view,
            format: VideoFrameFormat::Bgra,
            _textures: vec![wgpu_tex, dummy],
            _cv_metal_textures: vec![cv_tex],
        }
    }
}

/// Wrap two Metal textures (Y + CbCr) as wgpu textures via the Metal HAL.
#[cfg(target_os = "macos")]
fn wrap_metal_textures_as_wgpu(
    device: &wgpu::Device,
    mtl_y: &metal::Texture,
    y_format: wgpu::TextureFormat,
    y_width: u32,
    y_height: u32,
    mtl_cbcr: &metal::Texture,
    cbcr_format: wgpu::TextureFormat,
    cbcr_width: u32,
    cbcr_height: u32,
) -> (wgpu::Texture, wgpu::Texture) {
    let y_tex = wrap_single_metal_texture_as_wgpu(device, mtl_y, y_format, y_width, y_height);
    let cbcr_tex =
        wrap_single_metal_texture_as_wgpu(device, mtl_cbcr, cbcr_format, cbcr_width, cbcr_height);
    (y_tex, cbcr_tex)
}

/// Wrap a single Metal texture as a wgpu texture via the Metal HAL.
#[cfg(target_os = "macos")]
fn wrap_single_metal_texture_as_wgpu(
    device: &wgpu::Device,
    mtl_texture: &metal::Texture,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    use metal::foreign_types::ForeignType;

    // Clone the metal texture (increments ObjC refcount)
    let raw = unsafe { metal::Texture::from_ptr(mtl_texture.as_ptr()) };

    let hal_texture = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            raw,
            format,
            metal::MTLTextureType::D2,
            1, // array_layers
            1, // mip_levels
            wgpu::hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
        )
    };

    let desc = wgpu::TextureDescriptor {
        label: Some("surface_metal_import"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };

    unsafe { device.create_texture_from_hal::<wgpu::hal::metal::Api>(hal_texture, &desc) }
}

/// CPU-copy import for raw buffer frames.
fn import_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    planes: &[Vec<u8>],
    strides: &[u32],
    width: u32,
    height: u32,
    format: VideoFrameFormat,
) -> ImportedSurface {
    match format {
        VideoFrameFormat::Nv12 => {
            let y_tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("surface_y"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            queue.write_texture(
                y_tex.as_image_copy(),
                &planes[0],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(strides[0]),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );

            let cbcr_width = width / 2;
            let cbcr_height = height / 2;
            let cbcr_tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("surface_cbcr"),
                size: wgpu::Extent3d {
                    width: cbcr_width,
                    height: cbcr_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            queue.write_texture(
                cbcr_tex.as_image_copy(),
                &planes[1],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(strides[1]),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: cbcr_width,
                    height: cbcr_height,
                    depth_or_array_layers: 1,
                },
            );

            let y_view = y_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let cbcr_view = cbcr_tex.create_view(&wgpu::TextureViewDescriptor::default());

            ImportedSurface {
                y_view,
                cbcr_view,
                format,
                _textures: vec![y_tex, cbcr_tex],
                #[cfg(target_os = "macos")]
                _cv_metal_textures: vec![],
            }
        }
        VideoFrameFormat::Bgra => {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("surface_bgra"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            queue.write_texture(
                tex.as_image_copy(),
                &planes[0],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(strides[0]),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );

            let y_view = tex.create_view(&wgpu::TextureViewDescriptor::default());

            let dummy = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("surface_dummy_cbcr"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let cbcr_view = dummy.create_view(&wgpu::TextureViewDescriptor::default());

            ImportedSurface {
                y_view,
                cbcr_view,
                format,
                _textures: vec![tex, dummy],
                #[cfg(target_os = "macos")]
                _cv_metal_textures: vec![],
            }
        }
    }
}

/// CPU-copy import for a Linux DMA-BUF via mmap.
///
/// Maps the DMA-BUF fd into userspace, reads the plane data according to
/// the plane descriptors, and uploads via `queue.write_texture()`.
/// This is a fallback; zero-copy Vulkan external memory import can be added later.
#[cfg(target_os = "linux")]
fn import_dmabuf_cpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    fd: &std::sync::Arc<std::os::fd::OwnedFd>,
    dmabuf_planes: &[gpui::DmaBufPlane],
    width: u32,
    height: u32,
    format: gpui::VideoFrameFormat,
) -> ImportedSurface {
    use std::os::fd::AsRawFd;

    // Compute total size needed for mmap (max of all plane ends)
    let total_size = match format {
        VideoFrameFormat::Nv12 => {
            let y_end = dmabuf_planes[0].offset + dmabuf_planes[0].stride * height;
            let cbcr_end =
                dmabuf_planes[1].offset + dmabuf_planes[1].stride * (height / 2);
            y_end.max(cbcr_end) as usize
        }
        VideoFrameFormat::Bgra => {
            (dmabuf_planes[0].offset + dmabuf_planes[0].stride * height) as usize
        }
    };

    // Safety: We mmap the DMA-BUF fd read-only and read plane data from it.
    let mapped = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            total_size,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )
    };

    if mapped == libc::MAP_FAILED {
        log::error!("Failed to mmap DMA-BUF fd, returning dummy surface");
        // Return a minimal valid surface so we don't crash
        return import_buffer(device, queue, &[vec![0u8; 4]], &[4], 1, 1, VideoFrameFormat::Bgra);
    }

    let base = mapped as *const u8;

    let result = match format {
        VideoFrameFormat::Nv12 => {
            let y_size = (dmabuf_planes[0].stride * height) as usize;
            let y_data = unsafe {
                std::slice::from_raw_parts(
                    base.add(dmabuf_planes[0].offset as usize),
                    y_size,
                )
            };

            let cbcr_height = height / 2;
            let cbcr_size = (dmabuf_planes[1].stride * cbcr_height) as usize;
            let cbcr_data = unsafe {
                std::slice::from_raw_parts(
                    base.add(dmabuf_planes[1].offset as usize),
                    cbcr_size,
                )
            };

            import_buffer(
                device,
                queue,
                &[y_data.to_vec(), cbcr_data.to_vec()],
                &[dmabuf_planes[0].stride, dmabuf_planes[1].stride],
                width,
                height,
                format,
            )
        }
        VideoFrameFormat::Bgra => {
            let size = (dmabuf_planes[0].stride * height) as usize;
            let data = unsafe {
                std::slice::from_raw_parts(
                    base.add(dmabuf_planes[0].offset as usize),
                    size,
                )
            };

            import_buffer(
                device,
                queue,
                &[data.to_vec()],
                &[dmabuf_planes[0].stride],
                width,
                height,
                format,
            )
        }
    };

    unsafe {
        libc::munmap(mapped, total_size);
    }

    result
}

/// CPU-copy fallback for macOS CVPixelBuffer.
/// Used when the Metal texture cache is unavailable.
#[cfg(target_os = "macos")]
fn import_core_video_cpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cv_buf: &core_video::pixel_buffer::CVPixelBuffer,
) -> ImportedSurface {
    let width = cv_buf.get_width() as u32;
    let height = cv_buf.get_height() as u32;

    let _ = cv_buf.lock_base_address(0);

    let plane_count = cv_buf.get_plane_count();

    // Safety: We hold the lock on the pixel buffer, so the base address pointers are valid.
    let result = unsafe {
        if plane_count >= 2 {
            let y_ptr = cv_buf.get_base_address_of_plane(0);
            let y_stride = cv_buf.get_bytes_per_row_of_plane(0) as u32;
            let y_height = cv_buf.get_height_of_plane(0) as u32;
            let y_data =
                std::slice::from_raw_parts(y_ptr as *const u8, (y_stride * y_height) as usize);

            let cbcr_ptr = cv_buf.get_base_address_of_plane(1);
            let cbcr_stride = cv_buf.get_bytes_per_row_of_plane(1) as u32;
            let cbcr_height = cv_buf.get_height_of_plane(1) as u32;
            let cbcr_data = std::slice::from_raw_parts(
                cbcr_ptr as *const u8,
                (cbcr_stride * cbcr_height) as usize,
            );

            import_buffer(
                device,
                queue,
                &[y_data.to_vec(), cbcr_data.to_vec()],
                &[y_stride, cbcr_stride],
                width,
                height,
                VideoFrameFormat::Nv12,
            )
        } else {
            let ptr = cv_buf.get_base_address();
            let stride = cv_buf.get_bytes_per_row() as u32;
            let data =
                std::slice::from_raw_parts(ptr as *const u8, (stride * height) as usize);

            import_buffer(
                device,
                queue,
                &[data.to_vec()],
                &[stride],
                width,
                height,
                VideoFrameFormat::Bgra,
            )
        }
    };

    let _ = cv_buf.unlock_base_address(0);

    result
}
