//! Tiny zero-bloat Rust SDK for authoring capability-sandboxed Tomoyo WASM plugins
//! targeting `wasm32-unknown-unknown`.
//!
//! Use `export_tomoyo_wasm_plugin!(MyPlugin)` to export the standard `tomoyo_alloc`,
//! `tomoyo_free`, `tomoyo_query`, and `tomoyo_execute` ABI symbols cleanly.

pub use tomoyo_protocol::*;

pub trait TomoyoWasmPlugin {
    fn query(ctx: &QueryContext) -> Vec<ResultItem>;
    fn execute(_req: &ExecuteRequest) -> ExecuteResponse {
        ExecuteResponse::default()
    }
}

#[doc(hidden)]
pub fn __handle_query<P: TomoyoWasmPlugin>(ptr: i32, len: i32) -> i64 {
    let input = unsafe { read_guest_slice(ptr, len) };
    let items = match serde_json::from_slice::<QueryContext>(input) {
        Ok(ctx) => P::query(&ctx),
        Err(_) => Vec::new(),
    };
    let resp = QueryResponse { items };
    let bytes = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{\"items\":[]}".to_vec());
    pack_guest_vec(bytes)
}

#[doc(hidden)]
pub fn __handle_execute<P: TomoyoWasmPlugin>(ptr: i32, len: i32) -> i64 {
    let input = unsafe { read_guest_slice(ptr, len) };
    let resp = match serde_json::from_slice::<ExecuteRequest>(input) {
        Ok(req) => P::execute(&req),
        Err(_) => ExecuteResponse::default(),
    };
    let bytes = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{}".to_vec());
    pack_guest_vec(bytes)
}

#[doc(hidden)]
pub fn __alloc(len: i32) -> i32 {
    let len = len.max(0) as usize;
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr as usize as i32
}

#[doc(hidden)]
pub fn __free(ptr: i32, len: i32) {
    if ptr == 0 || len <= 0 {
        return;
    }
    let len = len as usize;
    unsafe {
        let _ = Vec::from_raw_parts(ptr as usize as *mut u8, 0, len);
    }
}

unsafe fn read_guest_slice<'a>(ptr: i32, len: i32) -> &'a [u8] {
    if ptr == 0 || len <= 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len as usize) }
}

fn pack_guest_vec(mut bytes: Vec<u8>) -> i64 {
    bytes.shrink_to_fit();
    let ptr = bytes.as_mut_ptr() as usize as u64;
    let len = bytes.len() as u64;
    std::mem::forget(bytes);
    ((ptr << 32) | (len & 0xFFFF_FFFF)) as i64
}

/// Safe wrapper around Tomoyo's capability-gated host imports.
#[cfg(target_arch = "wasm32")]
pub mod host {
    #[link(wasm_import_module = "tomoyo_host")]
    unsafe extern "C" {
        fn host_now_unix() -> i64;
        fn host_log(ptr: i32, len: i32);
    }

    pub fn now_unix() -> i64 {
        unsafe { host_now_unix() }
    }

    pub fn log(msg: &str) {
        unsafe { host_log(msg.as_ptr() as i32, msg.len() as i32) };
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    pub fn now_unix() -> i64 {
        0
    }
    pub fn log(_msg: &str) {}
}

/// Exports the Tomoyo WASM ABI entrypoints for a `TomoyoWasmPlugin` implementation.
#[macro_export]
macro_rules! export_tomoyo_wasm_plugin {
    ($plugin_ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn tomoyo_alloc(len: i32) -> i32 {
            $crate::__alloc(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn tomoyo_free(ptr: i32, len: i32) {
            $crate::__free(ptr, len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn tomoyo_query(ptr: i32, len: i32) -> i64 {
            $crate::__handle_query::<$plugin_ty>(ptr, len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn tomoyo_execute(ptr: i32, len: i32) -> i64 {
            $crate::__handle_execute::<$plugin_ty>(ptr, len)
        }
    };
}
