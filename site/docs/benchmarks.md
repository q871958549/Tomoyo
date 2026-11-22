# Tomoyo Launcher Verified Benchmarks & Hygiene Gates

All measurements below are reproducible directly from the repository using CI-enforced self-tests (`v2.3.0` Linux / `v2.2.6` Windows baseline).

## 1. Reproducible Verification Commands

```bash
# 7-gate repository hygiene, IPC parity, happy-dom smoke & release binary ceilings
node scripts/check_hygiene.mjs --check-binaries

# 100-cycle search / render / preview / hibernation / plugin reap soak test
./target/release/tomoyo --self-test --strict-budget
./target/release/tomoyo-webview-host --self-test
./target/release/tomoyo-capture --self-test
./target/release/tomoyo-plugin-ocr --self-test
```

## 2. Observed Results (`v2.3.0` Linux / `v2.2.6` Windows)

| Artifact / Metric | Linux Release (`--self-test`) | Windows Release (`v2.2.6`) | CI Hard Ceiling |
|---|---|---|---|
| `tomoyo` microkernel | 6.85 MB | 5.80 MB | <= 7.50 MB (Linux) / <= 6.50 MB (Win) |
| `tomoyo-webview-host` | 4.96 MB (0 MB idle) | 4.41 MB (0 MB idle) | <= 5.50 MB / 0 MB idle |
| `tomoyo-capture` | 1.46 MB (0 MB idle) | 1.15 MB (0 MB idle) | <= 1.50 MB / 0 MB idle |
| `tomoyo-plugin-ocr` / `tomoyo-plugin-notes` | 0.57 MB / 0.51 MB | 0.38 MB / 0.33 MB | <= 0.80 MB / 0 MB idle |
| Hibernated Private Commit | 22.35 MB (70 apps, SQLite, fonts) | 22.03 MB (343 entries; 1.50–1.52 MB WS) | <= 45.0 MB (Linux) / <= 28.0 MB (Win) |
| 100-cycle soak delta | Handles +0, Private +0.09 MB | GDI +0, USER +0, Handles +0, Private +0.08 MB | Handles <= +2, Private <= +0.50 MB |

## 3. Enforced 7-Gate Anti-Bloat Rules (`scripts/check_hygiene.mjs`)

1. Source file length ceiling: all 272 source files <= 450 lines (advisory warning > 380).
2. Zero forbidden heavy Rust crates (`tokio`, `reqwest`, `hyper`, `tauri`, `wry`, `async-std`).
3. Zero Direct2D / WARP bloat (`ID2D1DCRenderTarget`, `D2D1CreateFactory`).
4. Zero forbidden heavy npm packages (`react`, `vue`, `angular`, `antd`, `mui`, `lodash`, `axios`, `electron`) and strict bundle caps (`settings.html` <= 320 KB; `translator`/`clipboard`/`ocr` <= 150 KB).
5. Symmetric 1:1 Rust <-> TypeScript WebView IPC parity (47 commands and 27 `HostEvent` variants).
6. Real `happy-dom` JavaScript boot and interactive click smoke gate across all 4 compiled Svelte 5 bundles.
7. Release binary size ceiling enforcement (`--check-binaries`).
