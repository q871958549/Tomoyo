# Tomoyo Open Plugin Protocol (v3 · 100% v1/v2 Compatible)

Tomoyo plugins are language-agnostic, zero-resident-bloat extensions discovered from `./plugins/<plugin-dir>/tomoyo-plugin.toml` or `%LOCALAPPDATA%\Tomoyo\plugins/<plugin-dir>/tomoyo-plugin.toml` (`~/.local/share/tomoyo/plugins/` on Linux).

Protocol v3 (`PROTOCOL_VERSION = 3`) builds on v1/v2 with **Hybrid WebView ↔ STDIO/WASM RPC**, **OS-encrypted secret settings**, **dynamic system-tray actions**, **extension-aware `[[file_actions]]`**, **Super Panel `[[text_actions]]`**, and **`Permission::SelectionRead`** — enabling first-party and community tools alike to run with zero privilege asymmetry.

## 1. Plugin Manifest (`tomoyo-plugin.toml`)

```toml
id = "dev.example.my-plugin"
name = "My Plugin"
version = "3.1.0"
description = "Sample Protocol v3 Hybrid plugin"
author = "You"
license = "MIT"
icon = "icon.png"                # Optional <= 64 KB PNG icon
category = "productivity"        # productivity | ai | media | docs | dev | system
display_mode = "auto"            # "auto" | "inline" | "window"

# Permissions governed via Settings Center
# ("clipboard" | "network" | "filesystem_read" | "filesystem_write" | "selection_read" | "shell_exec")
permissions = ["clipboard", "selection_read"]

# Kernel events (`clipboard_text`, `clipboard_image`, `hotkey_triggered`)
events = ["hotkey_triggered"]

[runtime]
kind = "stdio_jsonrpc"           # "stdio_jsonrpc" | "wasm" | "webview"
command = "my-plugin"            # relative binary path or interpreter (e.g. "python")
args = []

# Optional Hybrid UI (Protocol v3): pairs a short-lived WebView frontend with the STDIO/WASM backend
[ui]
entry = "ui/index.html"
width = 780
height = 620
utools_compat = false

[triggers]
keywords = ["my"]                # triggered when user types `my <args>`
# regex = "^https?://"           # optional regex trigger

[resources]
max_memory_mb = 64               # Enforced via Windows Job Object / Linux cgroup-friendly limits
idle_timeout_secs = 60           # Automatically reaped when idle (0 MB idle RAM)
query_timeout_ms = 1500

# Declarative global hotkeys (governed by Tomoyo microkernel)
[[hotkeys]]
id = "selection_act"
label = "Act on Selection"
default_hotkey = ""              # Opt-in user binding in Settings Center
trigger_query = ""               # Empty dispatches `tomoyo/event` (`hotkey_triggered`)
capture_selection = true         # Requires `selection_read`; UIA-first zero-clipboard capture!

# Declarative system-tray actions (Protocol v3)
[[tray_actions]]
id = "open_ui"
label = "Open Workbench"
open_ui = true                   # Directly opens `[ui]` WebView without touching launcher

# Extension-aware file actions shown in Launcher Tab menu & Super Panel (Protocol v3)
[[file_actions]]
id = "inspect_md"
label = "Inspect Markdown"
extensions = [".md", "markdown"] # Use ["*"] for all files

# Contextual text actions surfaced in Tomoyo Super Panel (Protocol v3)
[[text_actions]]
id = "save_snippet"
label = "Save Selected Text"
match_kinds = ["text", "url", "json", "code", "color", "path"] # or ["any"]

# Declarative settings schema (auto-rendered in Settings Center -> Plugins)
[[settings]]
key = "api_key"
label = "API Key"
section = "Credentials"
type = "secret"                  # "text" | "bool" | "number" | "select" | "secret" | "hotkey" | "dir" | "file"

[[settings]]
key = "max_items"
label = "Max Items"
section = "General"
type = "number"
min = 5
max = 30
step = 5
default = 15
```

## 2. Wire Format (Newline-Delimited JSON-RPC 2.0 over Stdio)

Each request and response is a single UTF-8 JSON line terminated by `\n`.
Plugins MUST write protocol responses to `stdout` and diagnostic logs strictly to `stderr`.

### Methods

1. **`tomoyo/handshake`**
   - Request params: `{ "protocol_version": 3, "host_version": "3.1.0" }`
   - Result: `{ "protocol_version": 3, "name": "...", "version": "..." }`
2. **`tomoyo/query`**
   - Request params: `QueryContext` (`raw`, `trimmed`, `keyword`, `args`, `clipboard`, `attached`, `generation`, `limit`, `plugin_settings`, `plugin_secrets`)
   - `plugin_settings`: Manifest `[[settings]]` defaults merged with live overrides in `config.toml`.
   - `plugin_secrets`: Only `type = "secret"` fields explicitly declared by this plugin, decrypted from Windows DPAPI / Linux Secret Service strictly scoped to `plugin:<plugin_id>:<key>`.
   - Result: `{ "items": [ ResultItem, ... ] }`
3. **`tomoyo/execute`**
   - Request params: `{ "action_id": "...", "item_id": "...", "payload": { ... }, "plugin_settings": {}, "plugin_secrets": {} }`
   - Result: `ExecuteResponse` (`hide_window`, `toast`, `copy_text`, `open_url`, `fill_query`, `refresh_query`, `open_webview`, `view`)
4. **`tomoyo/event`**
   - Dispatched ONLY to enabled plugins that explicitly declare the event in `events = [...]` (and hold the required permission).
   - Request params: `{ "event": "clipboard_text" | "clipboard_image" | "hotkey_triggered", "data": { ... }, "plugin_settings": {}, "plugin_secrets": {} }`
   - Result: `ExecuteResponse`
5. **`tomoyo/call`** *(Hybrid WebView ↔ Backend RPC, Protocol v3)*
   - Invoked by `[ui]` pages via `await window.tomoyo.callPlugin(method, params)`, strictly sandboxed to the calling plugin's own `plugin_id`.
   - Request params: `{ "method": "...", "params": { ... }, "plugin_settings": {}, "plugin_secrets": {} }`
   - Result: Arbitrary JSON value returned directly to the WebView Promise.
6. **`tomoyo/shutdown`**
   - Request params: `{}`
   - Result: `{ "ok": true }` — plugin should flush and exit promptly.

## 3. Security, Governance & Anti-Leak Guarantees

- **Zero Resident Plugin Tax**: Plugins are never started until triggered by a keyword, hotkey, Super Panel action, or explicitly subscribed event, and are automatically reaped after `idle_timeout_secs` (0 MB idle RAM).
- **Zero-Clipboard UIA Selection Capture**: `Permission::SelectionRead` hotkeys and Super Panel probes read highlighted text via Windows UI Automation (`IUIAutomationTextPattern2`/`TextPattern`) first, never touching the OS clipboard or sending `Ctrl+C` into interactive terminals.
- **Centralized Hotkey Governance**: Plugins never install their own OS hooks. Declared `[[hotkeys]]` are registered by `tomoyo.exe` and automatically inherit conflict auto-rescue (`WH_KEYBOARD_LL`) and fullscreen/remote-desktop avoidance.
- **Job Object & WASM Isolation**: STDIO plugins run inside a Windows Job Object (`KILL_ON_JOB_CLOSE` + memory cap) or Linux process group (`PR_SET_PDEATHSIG`); WASM plugins run inside `wasmi` ABI v2 with fuel metering and isolated per-plugin KV storage.
- **Reference Implementations**: See `plugins/official-translate`, `plugins/official-clipboard`, `plugins/official-notes`, `plugins/official-capture`, `plugins/official-md-editor`, and `plugins/builtin-ocr` for complete Protocol v3 plugins.
- **Developer Handbook & Tooling**: See [`docs/PLUGIN_DEV_GUIDE.md`](PLUGIN_DEV_GUIDE.md) and [`docs/tomoyo.d.ts`](tomoyo.d.ts) for `tomoyo-cli new / dev --devtools / verify / pack / doctor` workflows and TypeScript definitions.
