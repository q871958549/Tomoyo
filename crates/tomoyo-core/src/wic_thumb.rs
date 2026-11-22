//! Zero-dependency Windows Imaging Component (WIC) bounded RGBA thumbnail decoder.
//! Supports PNG, JPEG, WebP, BMP, GIF, ICO, and TIFF on Windows without extra image crates.

use std::path::Path;

/// Returns `(src_w, src_h, dst_w, dst_h, rgba_bytes)` scaled to fit within `max_w x max_h`.
#[cfg(windows)]
pub fn decode_image_thumbnail_wic(
    path: &Path,
    max_w: u32,
    max_h: u32,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
        WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::Foundation::GENERIC_READ;

    let meta = std::fs::metadata(path).ok()?;
    if meta.len() == 0 || meta.len() > 32 * 1024 * 1024 {
        return None;
    }

    struct ComScope(bool);
    impl Drop for ComScope {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _com = ComScope(hr.is_ok());

        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let path_w = HSTRING::from(path.to_string_lossy().as_ref());
        let decoder = factory
            .CreateDecoderFromFilename(
                &path_w,
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;

        let mut src_w = 0u32;
        let mut src_h = 0u32;
        frame.GetSize(&mut src_w, &mut src_h).ok()?;
        if src_w == 0 || src_h == 0 || src_w > 12_000 || src_h > 12_000 {
            return None;
        }

        let scale = ((max_w as f32 / src_w as f32).min(max_h as f32 / src_h as f32)).min(1.0);
        let dst_w = ((src_w as f32 * scale).round() as u32).max(1);
        let dst_h = ((src_h as f32 * scale).round() as u32).max(1);

        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, dst_w, dst_h, WICBitmapInterpolationModeFant)
            .ok()?;

        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;

        let stride = dst_w.checked_mul(4)?;
        let buf_len = (stride as usize).checked_mul(dst_h as usize)?;
        let mut rgba = vec![0u8; buf_len];
        converter.CopyPixels(std::ptr::null(), stride, &mut rgba).ok()?;
        Some((src_w, src_h, dst_w, dst_h, rgba))
    }
}

#[cfg(target_os = "linux")]
pub fn decode_image_thumbnail_wic(
    path: &Path,
    max_w: u32,
    max_h: u32,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    crate::linux_sys::decode_image_rgba(path, max_w, max_h)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn decode_image_thumbnail_wic(
    _path: &Path,
    _max_w: u32,
    _max_h: u32,
) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    None
}
