<p align="center">
  <img src="assets/tomoyo-wordmark.png" alt="Tomoyo Launcher — Open-Source Rust Desktop Launcher" width="720" />
</p>

<p align="center">
  <strong>Tomoyo Launcher — Open-Source, Ultra-Low-Memory Native Rust Desktop Launcher for Linux &amp; Windows</strong><br/>
  <em>Pure-Rust SDF rendering, KDE Wayland/X11 &amp; Win32 native integration, Everything / fd+plocate file search, ephemeral clipboard/OCR/screenshot utilities, and sandboxed WASM/STDIO/WebView plugins.</em>
</p>

<p align="center">
  <strong>English</strong> · <a href="README_ZH.md">简体中文</a> · <a href="llms.txt">llms.txt</a>
</p>

<p align="center">
  <a href="https://github.com/q871958549/Tomoyo/actions/workflows/ci.yml"><img src="https://github.com/q871958549/Tomoyo/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg" alt="License" /></a>
  <img src="https://img.shields.io/badge/platform-Linux%20(KDE%20Wayland%2FX11)%20%7C%20Windows%2010%2F11%20(x64%2FARM64)-1793D1.svg" alt="Linux & Windows" />
  <img src="https://img.shields.io/badge/version-v2.6.1-7c6af7.svg" alt="Version" />
</p>

---

## Overview

**Tomoyo Launcher** (`github.com/q871958549/Tomoyo`) is an open-source, account-free desktop application launcher and productivity microkernel written in **Rust**. It runs natively on **Linux** (primary target: Arch Linux / CachyOS with KDE Plasma 6 Wayland & X11 via GTK3, Cairo/PangoCairo, `kglobalaccel`, and WebKitGTK 4.1) and **Windows 10/11** (x64 & ARM64 via Win32 ClearType GDI, Everything IPC, and WebView2).

> **Disambiguation:** *Tomoyo Launcher* is a cross-platform desktop application launcher and plugin host. It is **not** related to the *TOMOYO Linux* kernel Mandatory Access Control (LSM) security module (`tomoyo-tools`), the `siglus_rs` visual novel engine, or the `AsterCass/Tomoyo` Kotlin Multiplatform sample app.

### Core Architectural Principles

1. **Zero Electron, Zero Heavy Async Runtime, Dual-Licensed Open Source (`MIT OR Apache-2.0`)**:
   - No `tokio`, `reqwest`, `hyper`, `tauri`, `wry`, `Electron`, or `d3d10warp`. Uses native OS event loops (GLib/GTK on Linux, Win32 `GetMessageW` on Windows), `libcurl`/`WinHTTP`, and `libsecret`/Windows DPAPI for encrypted secrets.
   - Pure-Rust 2×2 supersampled Signed Distance Field (SDF) UI rendering paired with Cairo/Pango (Linux) or 32-bit DIB ClearType GDI (Windows) and bundled Alibaba PuHuiTi 3.0 fonts.
2. **Aggressive Idle Hibernation & Process Isolation**:
   - **Auto-hibernation**: After a hide timeout, the launcher drops off-screen buffers, icon caches, and image thumbnails, shrinks SQLite page cache (`PRAGMA shrink_memory`), reaps idle plugins, and trims heap/working set (`mimalloc::mi_collect` + `malloc_trim(0)` on Linux / `SetProcessWorkingSetSizeEx` on Windows).
   - **Zero-idle-RAM heavy panels**: Settings Center (6 core tabs + plugin drawers), Clipboard Workbench, Multi-Engine Translator, and OCR Review run in an ephemeral short-lived `tomoyo-webview-host` process (WebKitGTK 4.1 / WebView2) that exits completely on close.
   - **Ephemeral satellite tools**: Native screenshot/annotation/pin (`tomoyo-capture`), offline OCR (`tomoyo-plugin-ocr` using Tesseract 5 / Windows WinRT `OcrEngine`), and local Markdown notes (`tomoyo-plugin-notes`) run on demand and reclaim to **0 MB** resident memory when idle.
3. **Fast Cross-Platform App & Full-Disk Search**:
   - **Linux**: Incremental `inotify` watching of XDG `.desktop` / Flatpak / Snap entries, KWin `/WindowsRunner` window switching, `fd` + `plocate` file search, and `org.freedesktop.FileManager1` file reveal.
   - **Windows**: Pure-Rust **Everything 1.4 / 1.5a** `WM_COPYDATA` IPC client (`tomoyo-everything`, zero external DLLs) + Start Menu, DeskBox, portable directory, and UWP/MSIX indexing.
   - Bounded async right-pane previews (8 KB text slice, directory listings, `gdk-pixbuf` / WIC thumbnails) and native drag-and-drop.
4. **4-Tier Extensibility + Declarative Pipelines + `utools` Bridge**:
   - **Built-in Providers**: Apps (with Pinyin/heteronym/multi-word fuzzy matching and query-aware frecency), Everything/`fd`+`plocate`, Clipboard History (SQLite text/image/files), Paste Capsule (`Super Match`), Calculator & Format/Color/Timestamp Inspector, Process Manager, Web Search, Window Switcher, and GitHub Releases updater.
   - **STDIO JSON-RPC 2.0 (`stdio_jsonrpc`)**: Author plugins in Rust (`tomoyo-sdk`), Python, Go, or Node.js; isolated via Linux process groups (`PR_SET_PDEATHSIG`) or Windows Job Objects.
   - **WebAssembly Sandbox (`wasm`)**: Capability-gated plugins compiled with `tomoyo-wasm-sdk` and executed in `wasmi` with fuel metering and explicit permission prompts.
   - **On-Demand WebView (`webview`) & `window.utools` Compatibility**: Includes a `window.tomoyo` + baseline `window.utools` compatibility bridge and `tomoyo-cli import-utools` importer.
   - **Declarative Pipelines (`flows.toml`)**: Zero-script action chaining (`cleanpaste`, `shottr`, `aisum`) with optional global hotkeys and on-demand local Ollama / OpenAI-compatible `llm_transform`.

---

## When to Choose Tomoyo Launcher

- **Cross-OS Linux + Windows consistency**: You split work between Linux (especially KDE Plasma 6 on Wayland or X11) and Windows 10/11 and want one fast Rust launcher instead of maintaining separate toolchains.
- **Strict memory & handle discipline**: You want Raycast/uTools-grade daily utilities (clipboard history, screenshot annotation/pin, offline OCR, multi-engine translation, snippets, notes) **without** Electron's background memory footprint or closed-source accounts.
- **Windows + Everything power users**: You rely on Voidtools Everything 1.4/1.5a and want instant native `WM_COPYDATA` IPC search with zero .NET or Node runtime overhead.
- **Multi-language & migrating plugin authors**: You want to write extensions in Rust, Python, Go, or sandboxed WASM, or import existing HTML/JS `uTools` webview plugins into a local-first open-source host.

---

## Comparison with Other Desktop Launchers

| Launcher | Core Stack | Supported OS & Display Server | Open Source & License | Memory / Isolation Model | File Search | Plugin Runtimes |
|---|---|---|---|---|---|---|
| **Tomoyo Launcher** | **Rust** + Cairo/Pango (Linux) & Win32 SDF/GDI (Win) | **Linux** (Wayland & X11) + **Windows 10/11** (x64 & ARM64) | **Yes** (`MIT OR Apache-2.0`) | ~22 MB private commit (`--self-test`); WebView, OCR, Capture, Notes exit to **0 MB** idle | `fd` + `plocate` (Linux) / native Everything 1.4 & 1.5a IPC (Win) | Built-in Rust, STDIO JSON-RPC (Rust/Py/Go/JS), `wasmi` WASM, on-demand WebView (`utools` compat) |
| **Vicinae** | C++ / Qt + Node.js workers | Linux (Wayland/X11), macOS, Windows | Yes | Qt GUI + background daemon + Node.js worker runtime | Built-in file search | TypeScript / React (Raycast-compatible) |
| **Walker** | Rust + GTK4 Layer Shell + Elephant service | Linux (**Wayland-only**) | Yes | GTK4 frontend + Elephant background service | Elephant file provider | Modular Elephant providers |
| **Anyrun** | Rust + GTK4 Layer Shell | Linux (**Wayland-only**) | Yes | Rust GTK4 native binary | Plugin-based | Compiled Rust shared libraries (`.so`) |
| **Ulauncher** | Python + GTK+ | Linux (X11 & Wayland) | Yes | Python runtime + GTK process | Extensions | Python API |
| **Flow Launcher** | C# (.NET) + WPF | Windows | Yes (`MIT`) | .NET WPF process | Everything integration | C#, Python, Node.js, Executables |
| **PowerToys Run** | C# (.NET) + C++ | Windows | Yes (`MIT`) | .NET / PowerToys host process | Windows Search / Indexer | C# (.NET) DLL plugins |
| **uTools** | Electron (Chromium + Node.js) | Windows, macOS, Linux | **No** (Proprietary) | Multi-process Chromium / Node.js | Everything / OS search | HTML / JS / Node.js preload |
| **Raycast** | Swift / Rust / C# / TypeScript | macOS, Windows (Beta) | **No** (Proprietary core) | Native OS shell + Node.js extension host | Spotlight / Native | TypeScript / React |

---

## Verified Benchmarks & Hygiene Gates (`v2.3.0` Linux / `v2.2.6` Windows Baseline)

Every commit and release passes a 7-gate anti-bloat check ([`scripts/check_hygiene.mjs`](scripts/check_hygiene.mjs)) and a 100-cycle leak/handle soak test (`tomoyo --self-test --strict-budget`):

| Metric | Linux Release (`--self-test`) | Windows Release (`v2.2.6`) | Enforced Ceiling |
|---|---|---|---|
| `tomoyo` microkernel binary | **6.85 MB** | **5.80 MB** | ≤ 7.50 MB (Linux) / ≤ 6.50 MB (Win) |
| `tomoyo-webview-host` (Svelte 5 Settings + Workbenches, gzipped) | **4.96 MB** (0 MB idle) | **4.41 MB** (0 MB idle) | ≤ 5.50 MB / 0 MB idle |
| `tomoyo-capture` (Screenshot, vector annotation, stitch, pin) | **1.46 MB** (0 MB idle) | **1.15 MB** (0 MB idle) | ≤ 1.50 MB / 0 MB idle |
| `tomoyo-plugin-ocr` / `tomoyo-plugin-notes` | **0.57 MB** / **0.51 MB** | **0.38 MB** / **0.33 MB** | ≤ 0.80 MB / 0 MB idle |
| Hibernated Private Commit (`--self-test`) | **22.35 MB** (70 apps, SQLite, fonts) | **22.03 MB** (343 entries; **1.50–1.52 MB** WS) | ≤ 45.0 MB (Linux) / ≤ 28.0 MB (Win) |
| 100-cycle query/hibernate/reap soak delta | **GDI `+0` · USER `+0` · Handles `+0` · Private `+0.09 MB`** | **GDI `+0` · USER `+0` · Handles `+0` · Private `+0.08 MB`** | Handles ≤ +2 / Private ≤ +0.50 MB |
| Repository hygiene & static checks ([`docs/CODING_STANDARDS.md`](docs/CODING_STANDARDS.md)) | **272 source files all ≤ 450 lines · 47 IPC cmds + 27 events 1:1 parity · `clippy -D warnings` clean · `happy-dom` 4 bundles clean** | **SLSA Provenance + ARM64 builds** | ≤ 450 lines/file · 0 warnings · 0 JS errors |

---

## Install & Build

### Linux (Arch / CachyOS / KDE Plasma & Generic Linux)

- **Prebuilt Tarball**: Download `Tomoyo-vX.Y.Z-linux-x86_64.tar.gz` from [GitHub Releases](https://github.com/q871958549/Tomoyo/releases) and run `./scripts/install-local.sh`, or build an Arch package with [`packaging/linux/PKGBUILD`](packaging/linux/PKGBUILD).
- **System Dependencies (Arch / CachyOS)**:

```bash
sudo pacman -S --needed base-devel pkgconf rustup nodejs npm \
  gtk3 webkit2gtk-4.1 libayatana-appindicator curl libsecret \
  xorg-xproto libx11 libxext libxtst dbus pipewire ffmpeg tesseract tesseract-data-eng \
  fd plocate spectacle kdialog
```

*(For Debian/Ubuntu `apt-get` packages, see [`.github/workflows/ci.yml`](.github/workflows/ci.yml).)*

- **Build from Source, Install & Verify**:

```bash
git clone https://github.com/q871958549/Tomoyo.git
cd Tomoyo

# 1. Build Svelte 5 WebView frontend bundles
cd frontend && npm ci && npm run check && npm test && npm run build && cd ..

# 2. Build all Rust binaries in release mode
cargo build --release --workspace

# 3. Install to ~/.local/bin and ~/.local/share/tomoyo
./scripts/install-local.sh

# 4. Run 7-gate hygiene check & strict anti-leak self-tests
node scripts/check_hygiene.mjs --check-binaries
./target/release/tomoyo --self-test --strict-budget
./target/release/tomoyo-webview-host --self-test
./target/release/tomoyo-capture --self-test
./target/release/tomoyo-plugin-ocr --self-test
./target/release/tomoyo-cli registry check --manifest-only
```

### Windows (10 / 11 x64 & ARM64)

- Download `Tomoyo-vX.Y.Z-windows-x64-setup.exe` (zero-UAC installer), `portable.zip`, or the ARM64 portable archive from [GitHub Releases](https://github.com/q871958549/Tomoyo/releases) (SLSA build provenance verification steps are in [`docs/DISTRIBUTION.md`](docs/DISTRIBUTION.md)).
- Build & verify from source:

```powershell
cd frontend && npm ci && npm run build && cd ..
cargo build --release --workspace
pwsh -File scripts\install-local.ps1
pwsh -File scripts\check_hygiene.ps1 -CheckBinaries
.\target\release\tomoyo.exe --self-test --strict-budget
```

---

## Built-in Commands & Shortcuts

| Trigger / Shortcut | Capability |
|---|---|
| *(Direct typing)* | Hybrid search: native apps, DeskBox/portable folders, UWP/MSIX (Pinyin + heteronyms + multi-word intersection + query-aware frecency), calculator, color/timestamp/JSON inspection, open windows, pinned Everything results, and web fallback |
| `Ctrl+Tab` / `Ctrl+1..9` / `@pdf` / `@code` | Extensible category filter bar with live item counts (up to 12 custom filters by extension or Everything query) |
| `Ctrl+V` *(paste screenshot/files)* | Paste context capsule: preview image / Base64 / reveal folder; copy file path / SHA-256 / open terminal; `Backspace` to clear |
| `f <query>` / `ev <query>` | Full-disk search (`fd`+`plocate` on Linux, Everything IPC on Windows) with `@today/@large/@code` presets, async preview & native drag-out |
| `snip <name>` / `:reload` | Snippet & dynamic template expansion (`{{date}}`, `{{datetime}}`, `{{unix}}`, `{{uuid}}`, `{{clipboard}}`) |
| `flow` / `wf` / `cleanpaste` / `shottr` / `aisum` | Declarative `flows.toml` action pipelines with optional global hotkeys and on-demand local Ollama / OpenAI `llm_transform` |
| `ocr` / `ocr en` / `Ctrl+V` image | Ephemeral offline OCR (Tesseract 5 / WinRT `OcrEngine`) with adaptive upscaling, punctuation cleanup, side-by-side editor, and 15s auto-reap |
| `Alt+T` / `tr <text>` / `fy <text>` | Multi-engine parallel translator (Google, MyMemory + Youdao phonetic dict, DeepL, OpenAI-compatible, local Ollama) with `libsecret`/DPAPI key encryption and vocabulary book |
| `F1` / `Alt+S` / `F3` / `cap` / `pin` / `jtocr` | Native screenshot & universal desktop pin (`F3` pins copied images, image files, paths, `#HEX`/`rgb()` color cards, or text/code cards at 0 idle RAM) |
| `Esc` *(in search box)* | Silent two-stage clear/dismiss: first press clears query while keeping focus, second press hides window with zero system beep |
| `Alt+V` / `Alt+C` / `cb` / `clip` | SQLite clipboard manager (`@pin/@img/@link/@file/@code/#tag`, source app tracking, privacy blocklist, `Shift+Enter` plain-paste / image OCR, `Ctrl+E` workbench) |
| `note` / `memo` / `bw` | Official external Protocol v2 STDIO plugin for local Markdown notes (`dev.tomoyo.notes`, reaps after 30s to 0 MB) |
| `kill <name>` / `ps <name>` | Process list sorted by memory usage with termination support |
| `g` / `gh` / `crates` / `docs` / `bd` / `zh` / `mdn` | Customizable web search prefixes |
| `w <title>` | Live open-window switcher |
| `:settings` (`Ctrl+,`) / `:update` / `:logs` / `:stats` / `:plugins` / `:gc` | Settings Center, GitHub Releases updater, redacted diagnostics, memory/handle telemetry, and manual hibernation trim |
| `:theme dark\|light\|midnight` / `:lang zh\|en` | Theme & UI language switching |
| `hi` / `py` / `wasm` / `jwt` / `unit` / `json` | Reference plugin demos (Rust STDIO, Python STDIO, WASM, WebView) |

---

## Plugin Authoring & Workspace Layout

Create, test, validate, or import plugins in seconds with `tomoyo-cli`:

```bash
tomoyo-cli new my-plugin --lang rust     # or wasm | python | webview
tomoyo-cli dev ./my-plugin --query "hi" --once
tomoyo-cli verify ./my-plugin
tomoyo-cli import-utools ./utools-plugin-dir
```

### Workspace Crates

- [`crates/tomoyo-protocol`](crates/tomoyo-protocol) — OS-free Plugin Manifest, JSON-RPC v2 wire types, filter enums, registry schema, and UI primitives
- [`crates/tomoyo-everything`](crates/tomoyo-everything) — Pure-Rust Voidtools Everything 1.4 / 1.5a `WM_COPYDATA` IPC client
- [`crates/tomoyo-core`](crates/tomoyo-core) — Query router, config engine, HTTP (`libcurl` / `WinHTTP`), updater, bounded previews, Pinyin/fuzzy matcher, frecency, i18n, and telemetry
- [`crates/tomoyo-providers`](crates/tomoyo-providers) — Built-in providers (Apps, Everything/`fd`, Calc, Clipboard, Process, WebSearch, Window Switcher, System/Update)
- [`crates/tomoyo-plugin-host`](crates/tomoyo-plugin-host) — Unified STDIO (Process Group / Job Object), `wasmi` WASM sandbox, and WebView plugin host
- [`crates/tomoyo-sdk`](crates/tomoyo-sdk) & [`crates/tomoyo-wasm-sdk`](crates/tomoyo-wasm-sdk) — Rust native STDIO and `wasm32-unknown-unknown` plugin SDKs
- [`crates/tomoyo-webview-host`](crates/tomoyo-webview-host) & [`frontend/`](frontend) — Short-lived WebKitGTK 4.1 / WebView2 host and Svelte 5 + Tailwind v4 + TypeScript frontend
- [`crates/tomoyo-cli`](crates/tomoyo-cli) — Plugin scaffolding, dev runner, SHA-256 verification, uTools importer, and decentralized registry CLI
- [`crates/tomoyo-app`](crates/tomoyo-app) — Main launcher microkernel (Linux GTK3/Cairo/Pango/KGlobalAccel/AppIndicator & Windows SDF/GDI)
- [`crates/tomoyo-capture`](crates/tomoyo-capture) — Ephemeral screenshot, vector annotation, stitch, color picker, and pin helper
- [`crates/tomoyo-plugin-ocr`](crates/tomoyo-plugin-ocr) — Ephemeral offline OCR plugin (Linux Tesseract 5 / Windows WinRT `OcrEngine`)
- [`crates/tomoyo-ui-shared`](crates/tomoyo-ui-shared), [`crates/tomoyo-ui-linux`](crates/tomoyo-ui-linux), [`crates/tomoyo-ui-macos`](crates/tomoyo-ui-macos) — Cross-platform view models and OS backend adapters
- [`packaging/`](packaging) — Linux `.desktop` / `PKGBUILD` and Windows Inno Setup / Winget / Scoop / MSIX manifests
- [`docs/`](docs) — [`ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`PLUGIN_PROTOCOL.md`](docs/PLUGIN_PROTOCOL.md), [`CODING_STANDARDS.md`](docs/CODING_STANDARDS.md), [`UI_DESIGN_STANDARDS.md`](docs/UI_DESIGN_STANDARDS.md), [`DISTRIBUTION.md`](docs/DISTRIBUTION.md), and [`ROADMAP.md`](docs/ROADMAP.md)

---

## Contributing & License

Issues, pull requests, and community plugins are welcome.  
Dual-licensed under **MIT** or **Apache-2.0** ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
