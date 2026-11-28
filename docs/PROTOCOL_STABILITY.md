# Tomoyo Protocol v3 & WASM ABI v2 Stability Contract

## 1. Frozen Wire Contracts (`v3.5` / `v4.0` Baseline)
- **`tomoyo-protocol` (`PROTOCOL_VERSION = 3`)**：
  - JSON-RPC 2.0 STDIO 方法名冻结为 `tomoyo/handshake`、`tomoyo/query`、`tomoyo/execute`、`tomoyo/event`、`tomoyo/call`、`tomoyo/shutdown`。
  - 兼容承诺：所有 v1/v2 `PluginManifest` 与消息字段均保留 `#[serde(default)]` 向后兼容；后续 v3.x 次版本只允许向结构体新增带默认值的可选字段，**禁止**重命名或删除现有 JSON 键及 `ActionKind`/`PreviewSpec`/`SettingFieldKind` 变体标签。
- **`tomoyo-wasm-sdk` (`WASM_ABI_VERSION = 2`)**：
  - Guest 导出符号冻结：`tomoyo_alloc(i32) -> i32`、`tomoyo_free(i32, i32)`、`tomoyo_query(i32, i32) -> i64`、`tomoyo_execute(i32, i32) -> i64`（返回值按 `(ptr << 32) | len` 封包 UTF-8 JSON）。
  - Host 导入命名空间冻结（`tomoyo_host`）：`host_now_unix`、`host_now_ms`、`host_log`、`host_kv_get`、`host_kv_set`。

## 2. 自动化契约守门测试
- [`protocol_v3_freeze.rs`](../crates/tomoyo-protocol/tests/protocol_v3_freeze.rs)：验证 v1 manifest 降级解析、方法常量与全套 v3 线协议序列化往返。
- [`wasm_abi_v2_freeze.rs`](../crates/tomoyo-wasm-sdk/tests/wasm_abi_v2_freeze.rs)：验证 WASM 导出封包契约与宿主机导入存根。

## 3. `crates.io` 独立发布顺序
三件套 SDK 保持零 OS 重依赖，打包清单由 `cargo package --list -p ...` 在 CI 持续校验，发版发布按依赖拓扑顺序执行：
1. `cargo publish -p tomoyo-protocol`
2. `cargo publish -p tomoyo-sdk` 与 `cargo publish -p tomoyo-wasm-sdk`
