//! Bounded LRU Windows Shell `HICON` cache with deterministic RAII `DestroyIcon` cleanup.
//! Zero WIC/Direct2D dependencies — draws directly via `DrawIconEx` with full 32-bit alpha.

use lru::LruCache;
use std::num::NonZeroUsize;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

const MAX_CACHED_ICONS: usize = 96;

pub struct OwnedIcon(pub HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_invalid() {
                let _ = DestroyIcon(self.0);
            }
        }
    }
}

pub struct IconCache {
    cache: LruCache<String, Option<OwnedIcon>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(MAX_CACHED_ICONS).unwrap()),
        }
    }

    /// Destroys every cached `HICON` immediately when Tomoyo hibernates -> 0 icon handles held!
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn get_or_load(&mut self, path: &str) -> Option<HICON> {
        if let Some(cached) = self.cache.get(path) {
            return cached.as_ref().map(|o| o.0);
        }

        let icon = extract_shell_icon(path);
        let raw = icon.as_ref().map(|o| o.0);
        self.cache.put(path.to_string(), icon);
        raw
    }
}

fn extract_shell_icon(path: &str) -> Option<OwnedIcon> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut shfi = SHFILEINFOW::default();
        let res = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if res == 0 || shfi.hIcon.is_invalid() {
            return None;
        }
        Some(OwnedIcon(shfi.hIcon))
    }
}
