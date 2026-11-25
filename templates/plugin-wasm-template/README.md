# Tomoyo WASM Plugin Starter Template

10 分钟开发一个 **零权限、跨平台（Windows / Linux / macOS 一次编译通用）、16 MB 内存硬顶** 的 Tomoyo WebAssembly 插件。

## Quickstart / 快速上手

1. **安装 Rust WASM 目标**（仅首次）：
   ```bash
   rustup target add wasm32-unknown-unknown
   ```
2. **复制本模板为独立仓库**，将 `Cargo.toml` 中的 `tomoyo-wasm-sdk` 改为 Git 依赖：
   ```toml
   tomoyo-wasm-sdk = { git = "https://github.com/q871958549/Tomoyo", tag = "v3.2.0" }
   ```
3. **修改插件 ID 与逻辑**：编辑 `tomoyo-plugin.toml` 与 `src/lib.rs`。
4. **本地测试与编译**：
   ```bash
   cargo test
   cargo build --target wasm32-unknown-unknown --release
   cp target/wasm32-unknown-unknown/release/*.wasm plugin.wasm
   ```
5. **一键热调试**：
   ```bash
   tomoyo-cli dev . --query "hello" --once
   ```
6. **发布与提交社区市场**：
   打标签 `git tag v0.1.0 && git push origin v0.1.0` —— 内置的 `.github/workflows/release-plugin.yml` 会自动产出跨平台 ZIP 与 `SHA256SUMS.txt`，直接向 Tomoyo 主仓 `registry/` 提交 PR 即可被所有用户免登录一键安装。

## Built-in WASM ABI v2 Host Imports (`tomoyo_wasm_sdk::host`)

| API | Description |
| --- | --- |
| `host::now_unix()` / `host::now_ms()` | Host UTC clock (seconds / milliseconds) |
| `host::kv_get(key)` / `host::kv_set(key, val)` | Plugin-scoped persistent key-value storage (16 KB/value) |
| `host::log(msg)` | Structured debug trace forwarded to Tomoyo log |
| `PreviewSpec::RichCard` | Native declarative card UI rendered with 0 MB WebView cost |
