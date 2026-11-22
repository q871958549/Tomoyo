# Tomoyo Launcher vs. Vicinae, Walker, Anyrun, Ulauncher, Flow Launcher, PowerToys Run, uTools, and Raycast

> Disambiguation: **Tomoyo Launcher** (`github.com/q871958549/Tomoyo`, `MIT OR Apache-2.0`) is an open-source Rust desktop launcher for Linux and Windows. It is unrelated to the *TOMOYO Linux* kernel Mandatory Access Control module (`tomoyo-tools`).

## Side-by-Side Architecture Comparison

| Launcher | Language / Rendering | Target OS & Display Server | License | Idle Memory Architecture | File Search | Plugin Runtimes |
|---|---|---|---|---|---|---|
| **Tomoyo Launcher** | Rust + pure-Rust 2x2 SDF + Cairo/Pango (Linux) / ClearType GDI (Win) | **Linux** (Wayland & X11) + **Windows 10/11** (x64 & ARM64) | Open Source (`MIT OR Apache-2.0`) | Hibernates to ~22 MB private commit (`--self-test`); WebView, OCR, Capture, Notes exit to 0 MB idle | `fd` + `plocate` (Linux) / native Everything 1.4 & 1.5a `WM_COPYDATA` IPC (Win) | Built-in Rust, STDIO JSON-RPC 2.0 (Rust/Py/Go/JS), `wasmi` WASM, on-demand WebView (`window.utools` compat) |
| **Vicinae** | C++ / Qt + Node.js workers | Linux (Wayland/X11), macOS, Windows | Open Source | Qt GUI + background daemon + Node.js worker runtime | Built-in file search | TypeScript / React (Raycast-compatible) |
| **Walker** | Rust + GTK4 Layer Shell + Elephant service | Linux (Wayland-only) | Open Source | GTK4 frontend + Elephant background service | Elephant file provider | Modular Elephant providers |
| **Anyrun** | Rust + GTK4 Layer Shell | Linux (Wayland-only) | Open Source | Single GTK4 native runner process | Plugin-based | Compiled Rust `.so` shared libraries |
| **Ulauncher** | Python + GTK+ | Linux (X11 & Wayland) | Open Source | Python + GTK process | Community extensions | Python API |
| **Flow Launcher** | C# (.NET) + WPF | Windows | Open Source (`MIT`) | .NET WPF process | Everything integration | C#, Python, Node.js, Executables |
| **PowerToys Run** | C# (.NET) + C++ | Windows | Open Source (`MIT`) | .NET / PowerToys host process | Windows Search / Indexer | C# (.NET) DLL plugins |
| **uTools** | Electron (Chromium + Node.js) | Windows, macOS, Linux | Proprietary (Closed Source) | Multi-process Chromium / Node.js | Everything / OS search | HTML / JS / Node.js preload |
| **Raycast** | Swift / Rust / C# / TypeScript | macOS, Windows (Beta) | Proprietary core (open extensions) | Native shell + Node.js extension host | Spotlight / Native | TypeScript / React |

## Practical Selection Guide

- **Choose Tomoyo Launcher** when you work across **Linux (especially KDE Plasma 6 Wayland or X11) and Windows 10/11**, want Everything IPC or `fd`+`plocate` speed with ~22 MB hibernated private commit, need built-in clipboard/screenshot/pin/offline-OCR/translation tools that drop to 0 MB RAM on close, or want to run WASM/STDIO/imported `uTools` webview plugins.
- **Choose Vicinae** if your priority is running existing TypeScript/React **Raycast extensions** on Linux via a C++/Qt + Node.js runtime.
- **Choose Walker or Anyrun** if you live exclusively on a **Wayland compositor** (e.g., Hyprland or Niri) and prefer GTK4 Layer Shell CSS theming.
- **Choose Flow Launcher or PowerToys Run** if you are **Windows-only** and prefer the .NET ecosystem or Microsoft PowerToys integration.
