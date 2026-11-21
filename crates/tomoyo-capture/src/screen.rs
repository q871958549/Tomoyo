//! Per-Monitor V2 DPI-aware virtual desktop snapshot (`BitBlt` BGRA DIB) & window snap geometry.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ScreenRect {
    pub fn from_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let left = x0.min(x1);
        let top = y0.min(y1);
        let right = x0.max(x1);
        let bottom = y0.max(y1);
        Self {
            x: left,
            y: top,
            w: (right - left).max(0),
            h: (bottom - top).max(0),
        }
    }

    pub fn right(self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(self) -> i32 {
        self.y + self.h
    }

    pub fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    pub fn clamp_to(self, max_w: i32, max_h: i32) -> Self {
        let x0 = self.x.clamp(0, max_w);
        let y0 = self.y.clamp(0, max_h);
        let x1 = self.right().clamp(0, max_w);
        let y1 = self.bottom().clamp(0, max_h);
        Self {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0),
            h: (y1 - y0).max(0),
        }
    }

    pub fn is_valid(self) -> bool {
        self.w >= 4 && self.h >= 4
    }
}

#[derive(Debug, Clone)]
pub struct DesktopSnapshot {
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: i32,
    pub height: i32,
    /// Top-down 32-bit BGRA pixel buffer (`width * height * 4` bytes).
    pub bgra: Vec<u8>,
}

impl DesktopSnapshot {
    pub fn pixel_rgb(&self, x: i32, y: i32) -> (u8, u8, u8) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return (0, 0, 0);
        }
        let off = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        if off + 2 < self.bgra.len() {
            (self.bgra[off + 2], self.bgra[off + 1], self.bgra[off])
        } else {
            (0, 0, 0)
        }
    }

    pub fn crop_bgra(&self, rect: ScreenRect) -> Option<(u32, u32, Vec<u8>)> {
        let r = rect.clamp_to(self.width, self.height);
        if r.w <= 0 || r.h <= 0 {
            return None;
        }
        let mut out = vec![0u8; (r.w as usize) * (r.h as usize) * 4];
        let src_stride = (self.width as usize) * 4;
        let dst_stride = (r.w as usize) * 4;
        for row in 0..(r.h as usize) {
            let sy = (r.y as usize) + row;
            let src_off = sy * src_stride + (r.x as usize) * 4;
            let dst_off = row * dst_stride;
            out[dst_off..dst_off + dst_stride].copy_from_slice(&self.bgra[src_off..src_off + dst_stride]);
        }
        Some((r.w as u32, r.h as u32, out))
    }
}

/// Captures the entire Windows virtual desktop across all monitors in native physical pixels.
#[cfg(windows)]
pub fn capture_virtual_desktop() -> Result<DesktopSnapshot, String> {
    use std::ffi::c_void;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    unsafe {
        let origin_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let origin_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);

        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err("GetDC(screen) failed".into());
        }
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.is_invalid() {
            let _ = ReleaseDC(None, screen_dc);
            return Err("CreateCompatibleDC failed".into());
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let hbmp = match CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(h) if !h.is_invalid() && !bits.is_null() => h,
            _ => {
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(None, screen_dc);
                return Err("CreateDIBSection failed".into());
            }
        };

        let old = SelectObject(mem_dc, hbmp.into());
        let mut blit_res = BitBlt(
            mem_dc,
            0,
            0,
            width,
            height,
            Some(screen_dc),
            origin_x,
            origin_y,
            SRCCOPY | CAPTUREBLT,
        );
        if blit_res.is_err() {
            blit_res = BitBlt(
                mem_dc,
                0,
                0,
                width,
                height,
                Some(screen_dc),
                origin_x,
                origin_y,
                SRCCOPY,
            );
        }
        let blit_ok = blit_res.is_ok();

        let byte_len = (width as usize) * (height as usize) * 4;
        let bgra = if blit_ok {
            let mut buf = std::slice::from_raw_parts(bits as *const u8, byte_len).to_vec();
            for px in buf.chunks_exact_mut(4) {
                px[3] = 255;
            }
            buf
        } else {
            Vec::new()
        };

        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);

        if let Err(e) = blit_res {
            return Err(format!(
                "BitBlt virtual desktop ({origin_x},{origin_y} {width}x{height}) failed: {e}"
            ));
        }
        Ok(DesktopSnapshot {
            origin_x,
            origin_y,
            width,
            height,
            bgra,
        })
    }
}

/// Allocates and exercises a real 32-bit GDI `CreateDIBSection` offscreen surface and returns
/// a deterministic gradient snapshot for headless/locked-desktop CI self-tests.
#[cfg(windows)]
pub fn synthetic_dib_snapshot(width: i32, height: i32) -> Result<DesktopSnapshot, String> {
    use std::ffi::c_void;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    unsafe {
        let mem_dc = CreateCompatibleDC(None);
        if mem_dc.is_invalid() {
            return Err("CreateCompatibleDC failed".into());
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let hbmp = match CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(h) if !h.is_invalid() && !bits.is_null() => h,
            _ => {
                let _ = DeleteDC(mem_dc);
                return Err("CreateDIBSection failed".into());
            }
        };
        let old = SelectObject(mem_dc, hbmp.into());
        let len = (width as usize) * (height as usize) * 4;
        let slice = std::slice::from_raw_parts_mut(bits as *mut u8, len);
        for y in 0..height as usize {
            for x in 0..width as usize {
                let off = (y * width as usize + x) * 4;
                slice[off] = (x & 0xFF) as u8;
                slice[off + 1] = (y & 0xFF) as u8;
                slice[off + 2] = ((x + y) & 0xFF) as u8;
                slice[off + 3] = 255;
            }
        }
        let bgra = slice.to_vec();
        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(mem_dc);
        Ok(DesktopSnapshot { origin_x: 0, origin_y: 0, width, height, bgra })
    }
}

/// Enumerates visible top-level desktop window rectangles (in virtual-screen-relative coords).
#[cfg(windows)]
pub fn enumerate_snap_windows(origin_x: i32, origin_y: i32, max_w: i32, max_h: i32) -> Vec<ScreenRect> {
    use std::ffi::c_void;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible, GWL_EXSTYLE,
        WS_EX_TOOLWINDOW,
    };

    struct CollectCtx {
        origin_x: i32,
        origin_y: i32,
        max_w: i32,
        max_h: i32,
        rects: Vec<ScreenRect>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return BOOL(1);
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if (ex & WS_EX_TOOLWINDOW.0) != 0 {
                return BOOL(1);
            }
            let mut cloaked: u32 = 0;
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut c_void,
                std::mem::size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0
            {
                return BOOL(1);
            }
            let mut rc = RECT::default();
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut rc as *mut _ as *mut c_void,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_err()
                && GetWindowRect(hwnd, &mut rc).is_err()
            {
                return BOOL(1);
            }
            let ctx = &mut *(lparam.0 as *mut CollectCtx);
            let sr = ScreenRect {
                x: rc.left - ctx.origin_x,
                y: rc.top - ctx.origin_y,
                w: rc.right - rc.left,
                h: rc.bottom - rc.top,
            }
            .clamp_to(ctx.max_w, ctx.max_h);
            if sr.w >= 48 && sr.h >= 36 && ctx.rects.len() < 64 {
                ctx.rects.push(sr);
            }
            BOOL(1)
        }
    }

    let mut ctx = CollectCtx {
        origin_x,
        origin_y,
        max_w,
        max_h,
        rects: Vec::with_capacity(32),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_bgra_snapshot_accurately_with_bounds_clamping() {
        let mut bgra = vec![0u8; 4 * 4 * 4];
        // Mark pixel (2, 1) as RGB(10, 20, 30) -> BGRA(30, 20, 10, 255)
        let off = (4 + 2) * 4;
        bgra[off..off + 4].copy_from_slice(&[30, 20, 10, 255]);
        let snap = DesktopSnapshot {
            origin_x: -1920,
            origin_y: 0,
            width: 4,
            height: 4,
            bgra,
        };
        assert_eq!(snap.pixel_rgb(2, 1), (10, 20, 30));
        let (cw, ch, cropped) = snap.crop_bgra(ScreenRect { x: 1, y: 1, w: 5, h: 2 }).unwrap();
        assert_eq!((cw, ch), (3, 2));
        // (2, 1) becomes (1, 0) in cropped buffer
        assert_eq!(&cropped[4..8], &[30, 20, 10, 255]);
    }
}
