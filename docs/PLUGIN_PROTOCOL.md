# Tomoyo Open Plugin Protocol (v2 · 100% v1 Compatible)

Tomoyo plugins are language-agnostic, zero-resident-bloat extensions discovered from `./plugins/<plugin-dir>/tomoyo-plugin.toml` or `%LOCALAPPDATA%\Tomoyo\plugins/<plugin-dir>/tomoyo-plugin.toml`.

Protocol v2 (`PROTOCOL_VERSION = 2`) adds **declarative global hotkeys**, **permission-gated event subscriptions**, and **declarative typed settings schemas** while remaining 100% backward-compatible with v1 manifests.

## 1. Plugin Manifest (`tomoyo-plugin.toml`)

```toml
id = "dev.example.my-plugin"
name = "My Plugin"
version = "2.0.0"
description = "Sample Protocol v2 plugin"
author = "You"
license = "MIT"

# Optional permissions (prompted/governed via Settings Center)
permissions = ["clipboard"]

# Optional Protocol v2 kernel events (`clipboard_text`, `clipboard_image`, `hotkey_triggered`)
events = ["hotkey_triggered"]

[runtime]
kind = "stdio_jsonrpc"      # or "wasm" | "webview"
command = "python"          # or relative path to binary like "./my-plugin.exe"
args = ["main.py"]

[triggers]
keywords = ["my"]           # triggered when user types `my <args>`
# regex = "^https?://"      # optional regex trigger
# clipboard_types = ["text"]

[resources]
max_memory_mb = 64          # Enforced via Windows Job Object!
idle_timeout_secs = 60      # Automatically reaped when idle (0 MB idle RAM)
query_timeout_ms = 1500

# Optional Protocol v2 declarative global hotkeys (governed by Tomoyo microkernel)
# Inherits full Win32 RegisterHotKey + WH_KEYBOARD_LL conflict rescue + fullscreen/RayLink avoidance!
[[hotkeys]]
id = "quick_open"
label = "Quick Open My Plugin"
default_hotkey = ""         # Leave empty for opt-in user binding in Settings Center
trigger_query = "my "       # Non-empty fills launcher query; empty dispatches `tomoyo/event` (`hotkey_triggered`)

# Optional Protocol v2 declarative settings schema (auto-rendered in Settings Center -> Plugins)
[[settings]]
key = "max_items"
label = "Max Items"
description = "Maximum entries returned per query"
type = "number"             # "text" | "bool" | "number" | "select"
min = 5
max = 30
step = 5
default = 15

[[settings]]
key = "mode"
label = "Sort Mode"
type = "select"
default = "recent"
options = [
  { value = "recent", label = "Most Recent" },
  { value = "alpha", label = "Alphabetical" }
]

[[settings]]
key = "raw_output"
label = "Raw Output"
type = "bool"
default = false
```

## 2. Wire Format (Newline-Delimited JSON-RPC 2.0 over Stdio)

Each request and response is a single UTF-8 JSON line terminated by `\n`.
Plugins MUST write protocol responses to `stdout` and diagnostic logs strictly to `stderr`.

### Methods

1. **`tomoyo/handshake`**
   - Request params: `{ "protocol_version": 2, "host_version": "2.0.0" }`
   - Result: `{ "protocol_version": 2, "name": "...", "version": "..." }`
2. **`tomoyo/query`**
   - Request params: `QueryContext` (`raw`, `trimmed`, `keyword`, `args`, `clipboard`, `attached`, `generation`, `limit`, **`plugin_settings`**)
   - `plugin_settings`: JSON map containing manifest `[[settings]]` defaults merged with the user's live overrides from `config.toml` (`[plugin_settings."<plugin_id>"]`).
   - Result: `{ "items": [ ResultItem, ... ] }`
3. **`tomoyo/execute`**
   - Request params: `{ "action_id": "...", "item_id": "...", "payload": { ... } }`
   - Result: `ExecuteResponse` (`hide_window`, `toast`, `copy_text`, `open_url`, `fill_query`, `view`)
4. **`tomoyo/event`** *(New in Protocol v2)*
   - Dispatched ONLY to enabled plugins that explicitly declare the event in `events = [...]` (and hold `Permission::Clipboard` for clipboard events).
   - Request params: `{ "event": "clipboard_text" | "clipboard_image" | "hotkey_triggered", "data": { ... } }`
   - Result: `ExecuteResponse` (optional `toast` / `fill_query` handled by `tomoyo.exe`).
5. **`tomoyo/shutdown`**
   - Request params: `{}`
   - Result: `{ "ok": true }` — plugin should flush and exit promptly.

## 3. Security, Governance & Anti-Leak Guarantees

- **Zero Resident Plugin Tax**: Plugins are never started until triggered by a keyword, hotkey, or explicitly subscribed event, and are automatically reaped after `idle_timeout_secs` (0 MB idle RAM).
- **Centralized Hotkey Governance**: Plugins never install their own OS hooks. Declared `[[hotkeys]]` are registered by `tomoyo.exe` and automatically benefit from conflict auto-rescue (`WH_KEYBOARD_LL`) and fullscreen/remote-desktop avoidance.
- **Job Object & WASM Isolation**: STDIO plugins run inside a Windows Job Object with `KILL_ON_JOB_CLOSE` and `PROCESS_MEMORY` cap; WASM plugins run inside `wasmi` with fuel metering and zero ambient host access.
- **Reference Implementations**: See `plugins/official-notes` (`dev.tomoyo.notes`) and `plugins/builtin-ocr` (`dev.tomoyo.ocr`) for complete Protocol v2 plugins.
- **Developer Handbook & Tooling**: See [`docs/PLUGIN_DEV_GUIDE.md`](PLUGIN_DEV_GUIDE.md) and [`docs/tomoyo.d.ts`](tomoyo.d.ts) for `tomoyo-cli new / dev / verify / pack` workflows, Rust/WASM builders, and WebView TypeScript definitions.
