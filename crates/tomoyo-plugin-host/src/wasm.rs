//! Capability-sandboxed, fuel-metered WebAssembly plugin runtime powered by `wasmi`.
//!
//! Security & Anti-Leak Guarantees:
//! - Zero JIT/RWX executable memory reservations (compact pure-Rust interpreter).
//! - Hard instruction fuel ceiling (`max_fuel`) kills runaway infinite loops in milliseconds.
//! - Hard linear-memory cap (`StoreLimitsBuilder::memory_size`) prevents OOM balloons.
//! - Strict capability-based host imports: plugins have ZERO ambient fs/network access unless
//!   explicitly granted in `tomoyo-plugin.toml`.
//! - Dropped completely on idle reap -> 100% of linear memory freed immediately!

use std::io::Read;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tomoyo_protocol::{
    ExecuteRequest, ExecuteResponse, Permission, QueryContext, QueryResponse,
};
use wasmi::{Caller, Config, Engine, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

const MAX_HOST_READ_FILE_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum WasmError {
    #[error("failed to read WASM module `{0}`: {1}")]
    Io(String, std::io::Error),
    #[error("WASM compilation/validation error: {0}")]
    Compile(String),
    #[error("WASM runtime/trap error: {0}")]
    Runtime(String),
    #[error("WASM ABI error: {0}")]
    Abi(String),
    #[error("JSON serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

struct WasmHostState {
    plugin_id: String,
    permissions: Vec<Permission>,
    limits: StoreLimits,
}

pub struct LiveWasmPlugin {
    store: Store<WasmHostState>,
    instance: Instance,
    max_fuel: u64,
    last_used: Instant,
}

impl LiveWasmPlugin {
    pub fn load(
        plugin_id: &str,
        module_path: &Path,
        permissions: Vec<Permission>,
        max_memory_mb: u32,
        max_fuel: u64,
    ) -> Result<Self, WasmError> {
        let bytes = std::fs::read(module_path)
            .map_err(|e| WasmError::Io(module_path.display().to_string(), e))?;
        Self::from_bytes(plugin_id, &bytes, permissions, max_memory_mb, max_fuel)
    }

    pub fn from_bytes(
        plugin_id: &str,
        bytes: &[u8],
        permissions: Vec<Permission>,
        max_memory_mb: u32,
        max_fuel: u64,
    ) -> Result<Self, WasmError> {
        let mut cfg = Config::default();
        cfg.consume_fuel(true);
        let engine = Engine::new(&cfg);
        let module = Module::new(&engine, bytes).map_err(|e| WasmError::Compile(e.to_string()))?;

        let mem_bytes = (max_memory_mb.clamp(1, 256) as usize) * 1024 * 1024;
        let limits = StoreLimitsBuilder::new()
            .memory_size(mem_bytes)
            .instances(4)
            .tables(8)
            .build();

        let state = WasmHostState {
            plugin_id: plugin_id.to_string(),
            permissions,
            limits,
        };
        let mut store = Store::new(&engine, state);
        store.limiter(|s| &mut s.limits);
        store
            .set_fuel(max_fuel.max(100_000))
            .map_err(|e| WasmError::Runtime(e.to_string()))?;

        let mut linker = Linker::<WasmHostState>::new(&engine);
        register_host_imports(&mut linker)?;

        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| WasmError::Runtime(e.to_string()))?;

        Ok(Self {
            store,
            instance,
            max_fuel: max_fuel.max(100_000),
            last_used: Instant::now(),
        })
    }

    pub fn last_used(&self) -> Instant {
        self.last_used
    }

    /// Returns current allocated WebAssembly linear memory size in bytes.
    pub fn linear_memory_bytes(&self) -> u64 {
        self.instance
            .get_memory(&self.store, "memory")
            .map(|m| m.data_size(&self.store) as u64)
            .unwrap_or(0)
    }

    pub fn query(&mut self, ctx: &QueryContext) -> Result<QueryResponse, WasmError> {
        let payload = serde_json::to_vec(ctx)?;
        let out_bytes = self.invoke_json_abi("tomoyo_query", &payload)?;
        let resp: QueryResponse = serde_json::from_slice(&out_bytes)?;
        Ok(resp)
    }

    pub fn execute(&mut self, req: &ExecuteRequest) -> Result<ExecuteResponse, WasmError> {
        // `tomoyo_execute` is optional for pure-list WASM plugins.
        if self
            .instance
            .get_typed_func::<(i32, i32), i64>(&self.store, "tomoyo_execute")
            .is_err()
        {
            return Ok(ExecuteResponse::default());
        }
        let payload = serde_json::to_vec(req)?;
        let out_bytes = self.invoke_json_abi("tomoyo_execute", &payload)?;
        let resp: ExecuteResponse = serde_json::from_slice(&out_bytes)?;
        Ok(resp)
    }

    fn invoke_json_abi(&mut self, export_name: &str, input: &[u8]) -> Result<Vec<u8>, WasmError> {
        self.store
            .set_fuel(self.max_fuel)
            .map_err(|e| WasmError::Runtime(e.to_string()))?;

        let memory = self
            .instance
            .get_memory(&self.store, "memory")
            .ok_or_else(|| WasmError::Abi("missing exported `memory`".into()))?;
        let alloc = self
            .instance
            .get_typed_func::<i32, i32>(&self.store, "tomoyo_alloc")
            .map_err(|_| WasmError::Abi("missing exported `tomoyo_alloc(i32) -> i32`".into()))?;
        let free = self
            .instance
            .get_typed_func::<(i32, i32), ()>(&self.store, "tomoyo_free")
            .ok();
        let entry = self
            .instance
            .get_typed_func::<(i32, i32), i64>(&self.store, export_name)
            .map_err(|_| WasmError::Abi(format!("missing exported `{export_name}(i32, i32) -> i64`")))?;

        let in_len = input.len() as i32;
        let in_ptr = alloc
            .call(&mut self.store, in_len)
            .map_err(|e| WasmError::Runtime(format!("tomoyo_alloc trapped: {e}")))?;

        memory
            .write(&mut self.store, in_ptr as usize, input)
            .map_err(|e| WasmError::Abi(format!("memory write out of bounds: {e}")))?;

        let packed = entry
            .call(&mut self.store, (in_ptr, in_len))
            .map_err(|e| WasmError::Runtime(format!("{export_name} trapped: {e}")))?;

        if let Some(f) = &free {
            let _ = f.call(&mut self.store, (in_ptr, in_len));
        }

        let packed_u = packed as u64;
        let out_ptr = (packed_u >> 32) as u32 as usize;
        let out_len = (packed_u & 0xFFFF_FFFF) as u32 as usize;

        if out_len > 4 * 1024 * 1024 {
            return Err(WasmError::Abi("output payload exceeded 4 MB ceiling".into()));
        }

        let mut out_buf = vec![0u8; out_len];
        if out_len > 0 {
            memory
                .read(&self.store, out_ptr, &mut out_buf)
                .map_err(|e| WasmError::Abi(format!("memory read out of bounds: {e}")))?;
            if let Some(f) = &free {
                let _ = f.call(&mut self.store, (out_ptr as i32, out_len as i32));
            }
        }

        self.last_used = Instant::now();
        Ok(out_buf)
    }
}

fn register_host_imports(linker: &mut Linker<WasmHostState>) -> Result<(), WasmError> {
    linker
        .func_wrap("tomoyo_host", "host_now_unix", || -> i64 {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        })
        .map_err(|e| WasmError::Compile(e.to_string()))?;

    linker
        .func_wrap(
            "tomoyo_host",
            "host_log",
            |mut caller: Caller<'_, WasmHostState>, ptr: i32, len: i32| {
                let clamped = (len.max(0) as usize).min(1024);
                let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) else {
                    return;
                };
                let mut buf = vec![0u8; clamped];
                if mem.read(&mut caller, ptr as usize, &mut buf).is_ok() {
                    let msg = String::from_utf8_lossy(&buf);
                    tracing::info!(plugin = %caller.data().plugin_id, "[wasm] {msg}");
                }
            },
        )
        .map_err(|e| WasmError::Compile(e.to_string()))?;

    // Capability-gated file reader: returns -1 immediately unless `Permission::FilesystemRead` is granted!
    linker
        .func_wrap(
            "tomoyo_host",
            "host_read_file",
            |mut caller: Caller<'_, WasmHostState>,
             path_ptr: i32,
             path_len: i32,
             out_ptr: i32,
             max_len: i32|
             -> i32 {
                if !caller
                    .data()
                    .permissions
                    .contains(&Permission::FilesystemRead)
                {
                    return -1;
                }
                let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) else {
                    return -2;
                };
                let plen = (path_len.max(0) as usize).min(4096);
                let mut pbuf = vec![0u8; plen];
                if mem.read(&mut caller, path_ptr as usize, &mut pbuf).is_err() {
                    return -2;
                }
                let Ok(path_str) = std::str::from_utf8(&pbuf) else {
                    return -2;
                };
                let cap = (max_len.max(0) as usize).min(MAX_HOST_READ_FILE_BYTES);
                let Ok(mut f) = std::fs::File::open(path_str) else {
                    return -3;
                };
                let mut data = vec![0u8; cap];
                let Ok(n) = f.read(&mut data) else {
                    return -3;
                };
                if mem.write(&mut caller, out_ptr as usize, &data[..n]).is_err() {
                    return -2;
                }
                n as i32
            },
        )
        .map_err(|e| WasmError::Compile(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_fuel_meter_traps_infinite_loop() {
        let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "tomoyo_alloc") (param i32) (result i32) (i32.const 1024))
            (func (export "tomoyo_query") (param i32 i32) (result i64)
                (loop $inf (br $inf))
                (i64.const 0)
            )
        )
        "#;
        let mut plug = LiveWasmPlugin::from_bytes("test.loop", wat.as_bytes(), vec![], 4, 50_000)
            .expect("valid wat");
        let err = plug
            .query(&QueryContext::new("loop", 1, 5))
            .expect_err("infinite loop must trap on fuel exhaustion");
        assert!(
            err.to_string().to_lowercase().contains("fuel")
                || err.to_string().to_lowercase().contains("trap"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn wasm_capability_denies_fs_read_without_permission() {
        let wat = r#"
        (module
            (import "tomoyo_host" "host_read_file" (func $read (param i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "{\"items\":[]}")
            (func (export "tomoyo_alloc") (param i32) (result i32) (i32.const 512))
            (func (export "tomoyo_query") (param i32 i32) (result i64)
                ;; Attempt unauthorized read; if it returns -1 (denied), return valid empty JSON at (0, 12).
                (if (i32.eq (call $read (i32.const 512) (i32.const 4) (i32.const 1024) (i32.const 64)) (i32.const -1))
                    (then (return (i64.const 12)))
                )
                (unreachable)
            )
        )
        "#;
        let mut plug = LiveWasmPlugin::from_bytes("test.cap", wat.as_bytes(), vec![], 4, 200_000)
            .expect("valid wat");
        let resp = plug
            .query(&QueryContext::new("cap", 1, 5))
            .expect("permission denial must return -1 safely");
        assert!(resp.items.is_empty());
    }
}
