# Tomoyo Launcher Plugin Developer Guide

Build ultra-lightweight plugins for **Tomoyo Launcher** (`Linux` & `Windows`) with **0 MB idle memory overhead**.

---

## 1. Choose a Plugin Runtime

| Runtime | `[runtime].kind` | Best For | Memory & Isolation |
|---|---|---|---|
| **WebView** | `"webview"` | Markdown editors, JSON/regex tools, API testers, rich UIs, migrating uTools plugins | Ephemeral `tomoyo-webview-host` process (0 MB when closed), includes `window.tomoyo` + optional `window.utools` + embedded SQLite `db` / `dbStorage` |
| **WASM Sandbox** | `"wasm"` | Calculators, unit/hash/JWT tools, formatters, pure transforms | `wasmi` fuel-metered sandbox (`max_fuel` + memory cap), zero ambient FS/network access unless granted |
| **STDIO JSON-RPC (Rust)** | `"stdio_jsonrpc"` | Fast local indexers (`official-notes`), CLI wrappers, custom workflows | Native compiled binary using [`crates/tomoyo-sdk`](../crates/tomoyo-sdk/src/lib.rs); reaped automatically after `idle_timeout_secs` |
| **STDIO JSON-RPC (Python/Node/Go)** | `"stdio_jsonrpc"` | Rapid scripts, AI/API helpers, existing automation | Any language speaking newline-delimited JSON-RPC 2.0 on stdin/stdout; Linux process group + `PR_SET_PDEATHSIG` / Windows Job Object |

---

## 2. 30-Second Quickstart (`tomoyo-cli`)

```bash
# 1. Scaffold a starter project (works inside OR outside the Tomoyo repo!)
tomoyo-cli new my-markdown --lang webview --dir ~/tomoyo-plugins
# Or Rust / WASM / Python:
tomoyo-cli new my-calc --lang wasm --dir ~/tomoyo-plugins

# 2. Validate manifest, entrypoints, WASM magic, duplicate IDs & live smoke query
cd ~/tomoyo-plugins/my-calc
tomoyo-cli verify .

# 3. Dev watch loop with auto Cargo build, query/execute/event testing & live reload signal
tomoyo-cli dev . --build --query "2+2"
tomoyo-cli dev . --once --exec greet --data "world"
tomoyo-cli dev . --once --event hotkey_triggered

# 4. Package a reproducible ZIP archive + compute SHA-256 + print registry/index.json snippet
tomoyo-cli pack .
```

> **Note:** Outside the Tomoyo repository, `tomoyo-cli new --lang rust|wasm` automatically binds `tomoyo-sdk` / `tomoyo-wasm-sdk` to `git = "https://github.com/q871958549/Tomoyo"`. Pass `--sdk-dir /path/to/Tomoyo/crates` if developing against a local clone.

---

## 3. Ergonomic Rust / WASM SDK Helpers

Both `tomoyo_sdk::*` and `tomoyo_wasm_sdk::*` re-export `tomoyo_protocol::*` builders:

```rust
use tomoyo_sdk::*;

struct CalcPlugin;
impl TomoyoPlugin for CalcPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("Sci Calc", "0.1.0")
    }

    fn query(&self, ctx: &QueryContext) -> Vec<ResultItem> {
        let expr = ctx.input_or("1+1");
        let prec = ctx.setting_i64("precision", 4);
        let ans = format!("{expr} (prec={prec})");
        vec![
            ResultItem::plugin("calc:ans", "dev.author.calc", &ans, "Copy result")
                .with_monogram("=")
                .with_badge("CALC")
                .with_copy(&ans)
                .with_action(ActionSpec::copy(&ans))
                .with_action(ActionSpec::copy_and_paste(&ans))
                .with_kv_preview("Breakdown", vec![("Expression".into(), expr.into()), ("Answer".into(), ans.clone())]),
        ]
    }
}
```

---

## 4. WebView API & TypeScript Intellisense

Every `webview` plugin scaffolded via `tomoyo-cli new --lang webview` includes [`docs/tomoyo.d.ts`](tomoyo.d.ts) locally so VS Code / IDEs provide full autocomplete for:

- `window.tomoyo.onEnter((action) => ...)` — receives initial query + reactivation payloads
- `window.tomoyo.isDarkColors()` — sync CSS light/dark tokens with Tomoyo's active theme (`light` / `dark` / `midnight`)
- `window.tomoyo.db` & `window.tomoyo.dbStorage` — per-plugin persisted KV/document storage (SQLite-backed)
- `window.tomoyo.copyText(text)`, `togglePinTopmost()`, `openExternal(url)`, `openPath(path)`, `close()`
- `window.utools` (`utools_compat = true`) — baseline compatibility for importing existing uTools HTML/JS plugins (`tomoyo-cli import-utools <dir>`)

---

## 5. Protocol v2 Capabilities (`tomoyo-plugin.toml`)

See [`docs/PLUGIN_PROTOCOL.md`](PLUGIN_PROTOCOL.md) for the full wire schema:

- **`[[hotkeys]]`**: Declare global shortcuts governed centrally by Tomoyo (KGlobalAccel / RegisterHotKey + conflict rescue + fullscreen game avoidance).
- **`[[settings]]`**: Declare `text` / `bool` / `number` / `select` fields auto-rendered in **Settings Center → Plugins → Configure**, injected into `QueryContext.plugin_settings`.
- **`events = ["clipboard_text", "clipboard_image", "hotkey_triggered"]`**: Opt-in non-blocking background events.

---

## 6. Contributing a Plugin to the Community Registry

1. Run `tomoyo-cli verify .` and `tomoyo-cli pack .`.
2. Publish your `.zip` archive (e.g. as a GitHub Release asset) OR place standalone plugins in `plugins/`.
3. Open a PR updating [`registry/index.json`](../registry/index.json) with the JSON snippet printed by `tomoyo-cli pack .` (verified automatically in CI by `tomoyo-cli registry check`).
4. For issues/submissions, you can also use `.github/ISSUE_TEMPLATE/plugin_submission.yml`.
