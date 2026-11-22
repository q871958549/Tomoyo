# Tomoyo Launcher — AI Visibility & Ecosystem Distribution Playbook (GEO)

This checklist covers the external indexing, package registry, and high-authority reference steps needed so generative search engines (Gemini, ChatGPT Search, Perplexity, Claude) can discover, disambiguate, and accurately recommend **Tomoyo Launcher** (`q871958549/Tomoyo`).

---

## 1. Repository & GitHub Pages Activation (After Pushing `main`)

1. **Commit and push** the updated `README.md`, `README_ZH.md`, `llms.txt`, `llms-full.txt`, `site/`, and `.github/workflows/pages.yml` to `main`.
2. **Enable GitHub Pages** (Settings -> Pages -> Source: **GitHub Actions**), or via CLI after push:
   ```bash
   gh api -X POST repos/q871958549/Tomoyo/pages -f build_type=workflow
   ```
3. **Set GitHub Homepage URL** once `https://q871958549.github.io/Tomoyo/` returns HTTP 200:
   ```bash
   gh repo edit q871958549/Tomoyo --homepage "https://q871958549.github.io/Tomoyo/"
   ```
4. **Submit Sitemap to Both Major Grounding Indexes**:
   - **Google Search Console** (feeds Google Search & Gemini Grounding): verify `https://q871958549.github.io/Tomoyo/` and submit `sitemap.xml`.
   - **Bing Webmaster Tools** (feeds Bing Search & ChatGPT Search): import from Google Search Console or verify and submit `sitemap.xml`.

---

## 2. Publish Core SDK Crates to `crates.io` + `docs.rs`

All four names (`tomoyo-protocol`, `tomoyo-everything`, `tomoyo-sdk`, `tomoyo-wasm-sdk`) are currently available and pre-configured with `repository`, `homepage`, `readme`, `keywords`, and `categories`.

Publish in dependency order (after committing and tagging):

```bash
cargo publish -p tomoyo-protocol
cargo publish -p tomoyo-everything
cargo publish -p tomoyo-sdk
cargo publish -p tomoyo-wasm-sdk
```

*Why this matters for LLMs*: `crates.io` and `docs.rs` pages are high-authority, structured Rust ecosystem nodes that search crawlers index quickly, and publishing `tomoyo-sdk` / `tomoyo-protocol` resolves namespace overlap with `AsterCass/Tomoyo`.

---

## 3. Package Managers (Authoritative Structured Indexes)

- **AUR (`tomoyo-launcher` / `tomoyo-launcher-bin`)**:
  - Avoid `tomoyo-tools` (already used by TOMOYO Linux) and `tomoyo-explorer-bin`.
  - Publish [`packaging/linux/PKGBUILD`](../packaging/linux/PKGBUILD) under `tomoyo-launcher` (with `provides=('tomoyo')` if desired) after the Linux release tag is live.
- **Winget (`microsoft/winget-pkgs`)**:
  - Use `wingetcreate` per [`docs/DISTRIBUTION.md`](DISTRIBUTION.md) with [`packaging/winget/`](../packaging/winget).
- **Scoop (`ScoopInstaller/Extras`)**:
  - Host a personal bucket first (`scoop install https://github.com/q871958549/Tomoyo/releases/download/vX.Y.Z/tomoyo.scoop.json`), then PR to `ScoopInstaller/Extras` once repository stars meet Extras criteria.

---

## 4. High-Authority Directory & Community Co-Occurrence

### A. AlternativeTo Submission Copy (Directly feeds "alternative to X" LLM queries)

- **Name**: `Tomoyo Launcher`
- **Website**: `https://q871958549.github.io/Tomoyo/`
- **Source**: `https://github.com/q871958549/Tomoyo`
- **License**: `MIT / Apache-2.0`
- **Platforms**: `Linux`, `Windows`
- **Alternative To**: `Raycast`, `Flow Launcher`, `uTools`, `Ulauncher`, `Walker`, `PowerToys Run`, `Alfred`
- **Summary**:
  > Open-source, ultra-low-memory native Rust desktop launcher for Linux (KDE Plasma Wayland/X11, GTK3) and Windows 10/11. Uses pure-Rust 2x2 SDF rendering with Cairo/Win32 GDI (no Electron or tokio), integrates Everything 1.4/1.5a IPC and fd/plocate, hibernates to ~22 MB private commit, and ships on-demand 0-idle-RAM clipboard, screenshot/pin, offline OCR, translation, and STDIO/WASM/WebView (`window.utools` compatible) plugins.

### B. Show HN / Reddit Technical Architecture Post Template

Submit only after `main` and a clean Linux + Windows Release tag are live:

- **Title**:
  `Show HN: Tomoyo Launcher – 22 MB RAM native Rust desktop launcher (Linux Wayland/X11 & Windows) with Everything IPC and WASM plugins`
- **Technical Body Focus** *(avoid marketing tone; highlight verifiable engineering decisions)*:
  - Why it avoids `Electron`, `Tauri`, `tokio`, `reqwest`, and `Direct2D/WARP`, using GLib/GTK3 + Cairo/Pango (`kglobalaccel` D-Bus / X11 fallback) on Linux and Win32 ClearType GDI + `RegisterHotKey` on Windows with pure-Rust 2x2 SDF UI rendering.
  - How heavy tools (`tomoyo-webview-host` via WebKitGTK 4.1 / WebView2, `tomoyo-capture` screenshot/pin, `tomoyo-plugin-ocr` Tesseract 5 / WinRT OCR, `tomoyo-plugin-notes`) stay at 0 MB resident memory when closed, backed by `tomoyo --self-test --strict-budget` (100 soak cycles, zero handle leaks) and `check_hygiene.mjs` (<= 450 lines/file).
  - Why it includes 4 plugin runtimes (Built-in Rust, STDIO JSON-RPC 2.0, `wasmi` fuel-metered WASM, and ephemeral WebView with baseline `window.utools` compatibility).

### C. `Awesome-*` Lists (Only submit PRs after meeting each repo's minimum star/age rules)

- `awesome-rust` (Applications / Utilities)
- `awesome-wayland` (Launchers)
- `awesome-linux-software` / `awesome-windows`
