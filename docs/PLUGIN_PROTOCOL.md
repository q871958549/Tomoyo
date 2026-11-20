# Tomoyo Open Plugin Protocol (v1)

Tomoyo plugins are language-agnostic extensions discovered from `./plugins/<plugin-dir>/tomoyo-plugin.toml` or `%LOCALAPPDATA%\Tomoyo\plugins/<plugin-dir>/tomoyo-plugin.toml`.

## 1. Plugin Manifest (`tomoyo-plugin.toml`)

```toml
id = "dev.example.my-plugin"
name = "My Plugin"
version = "0.1.0"
description = "Sample plugin description"
author = "You"
license = "MIT"

[runtime]
kind = "stdio_jsonrpc"
command = "python"          # or relative path to binary like "./my-plugin.exe"
args = ["main.py"]

[triggers]
keywords = ["my"]           # triggered when user types `my <args>`
# regex = "^https?://"      # optional regex trigger
# clipboard_types = ["text"]

[resources]
max_memory_mb = 64          # Enforced via Windows Job Object!
idle_timeout_secs = 60      # Automatically reaped when idle
query_timeout_ms = 1500
```

## 2. Wire Format (Newline-Delimited JSON-RPC 2.0 over Stdio)

Each request and response is a single UTF-8 JSON line terminated by `\n`.
Plugins MUST write protocol responses to `stdout` and diagnostic logs strictly to `stderr`.

### Methods

1. **`tomoyo/handshake`**
   - Request params: `{ "protocol_version": 1, "host_version": "0.1.0" }`
   - Result: `{ "protocol_version": 1, "name": "...", "version": "..." }`
2. **`tomoyo/query`**
   - Request params: `QueryContext` (`raw`, `trimmed`, `keyword`, `args`, `clipboard`, `generation`, `limit`)
   - Result: `{ "items": [ ResultItem, ... ] }`
3. **`tomoyo/execute`**
   - Request params: `{ "action_id": "...", "item_id": "...", "payload": { ... } }`
   - Result: `ExecuteResponse` (`hide_window`, `toast`, `copy_text`, `open_url`, `fill_query`, `view`)
4. **`tomoyo/shutdown`**
   - Request params: `{}`
   - Result: `{ "ok": true }` — plugin should flush and exit promptly.

## 3. Security & Anti-Leak Guarantees

- **Lazy Spawn**: Plugins are never started until triggered.
- **Job Object Enforcement**: Every plugin process runs inside an OS Job Object with `KILL_ON_JOB_CLOSE` and `PROCESS_MEMORY` cap.
- **Idle Reaping**: Inactive plugin processes are terminated automatically after `idle_timeout_secs`.
