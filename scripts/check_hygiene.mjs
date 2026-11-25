#!/usr/bin/env node
// Cross-platform Tomoyo Repository Hygiene & Anti-Bloat Gate (7 Gates).
// Mirrors scripts/check_hygiene.ps1 for Linux/macOS/Windows CI and local development.

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const args = process.argv.slice(2);
const checkBinaries = args.includes("--check-binaries");
const maxLines = Number(args.find((a) => a.startsWith("--max-lines="))?.split("=")[1] ?? 450);
const warnLines = Number(args.find((a) => a.startsWith("--warn-lines="))?.split("=")[1] ?? 380);
const totalSteps = checkBinaries ? 7 : 6;
let violations = 0;

function walk(dir, filterFn, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    if (["target", "__pycache__", "node_modules", "dist", ".git"].includes(ent.name)) continue;
    const full = path.join(dir, ent.name);
    if (ent.isDirectory()) walk(full, filterFn, out);
    else if (filterFn(full)) out.push(full);
  }
  return out;
}

function countLines(filePath) {
  const raw = fs.readFileSync(filePath, "utf8");
  if (raw.length === 0) return 0;
  const lines = raw.split(/\r?\n/);
  return raw.endsWith("\n") ? lines.length - 1 : lines.length;
}

console.log(`=== [1/${totalSteps}] Checking source file line-count ceiling (warn > ${warnLines}, hard fail > ${maxLines}) ===`);
const srcExts = new Set([".rs", ".c", ".h", ".html", ".css", ".js", ".mjs", ".py", ".ts", ".svelte"]);
const srcFiles = ["crates", "plugins", "scripts", "frontend/src", "frontend/scripts", "frontend/tests"]
  .flatMap((root) => walk(root, (f) => srcExts.has(path.extname(f))));

const softWarnings = [];
let lineFails = 0;
for (const file of srcFiles) {
  const n = countLines(file);
  if (n > maxLines) {
    console.error(`  [FAIL] ${file} has ${n} lines (limit: ${maxLines})`);
    violations++;
    lineFails++;
  } else if (n > warnLines) {
    softWarnings.push({ file, lines: n });
  }
}
if (lineFails === 0) {
  console.log(`  [OK] All ${srcFiles.length} source files are strictly <= ${maxLines} lines!`);
  if (softWarnings.length > 0) {
    console.log(`  [INFO] ${softWarnings.length} file(s) exceed soft advisory watermark (${warnLines} lines):`);
    for (const w of softWarnings.sort((a, b) => b.lines - a.lines)) {
      console.log(`    - ${w.file} (${w.lines} lines)`);
    }
  }
}

console.log(`=== [2/${totalSteps}] Checking forbidden heavy dependencies in Cargo.toml ===`);
const forbiddenCrateRe = /^\s*(tokio|reqwest|hyper|tauri|wry|async-std)\s*=/gm;
const tomls = [
  ...(fs.existsSync("Cargo.toml") ? ["Cargo.toml"] : []),
  ...walk("crates", (f) => path.basename(f) === "Cargo.toml"),
  ...walk("plugins", (f) => path.basename(f) === "Cargo.toml"),
];
let depViolations = 0;
for (const t of tomls) {
  const raw = fs.readFileSync(t, "utf8");
  for (const m of raw.matchAll(forbiddenCrateRe)) {
    console.error(`  [FAIL] Forbidden heavy crate in ${t}: ${m[0].trim()}`);
    depViolations++;
  }
}
violations += depViolations;
if (depViolations === 0) console.log("  [OK] Zero forbidden heavy crates detected.");

console.log(`=== [3/${totalSteps}] Checking anti-WARP / anti-Direct2D regression ===`);
const rustFiles = walk("crates", (f) => f.endsWith(".rs"));
let warpViolations = 0;
for (const rf of rustFiles) {
  const raw = fs.readFileSync(rf, "utf8");
  if (/ID2D1DCRenderTarget|D2D1CreateFactory/.test(raw)) {
    console.error(`  [FAIL] Forbidden D2D/WARP API in ${rf}`);
    warpViolations++;
  }
}
violations += warpViolations;
if (warpViolations === 0) console.log("  [OK] Zero D2D/WARP bloat detected.");

console.log(`=== [4/${totalSteps}] Checking frontend bundle size budgets & forbidden heavy npm packages ===`);
let feViolations = 0;
if (fs.existsSync("frontend/package.json")) {
  const pkgRaw = fs.readFileSync("frontend/package.json", "utf8");
  const forbiddenNpm = ['"react"', '"react-dom"', '"vue"', '"@angular/core"', '"antd"', '"@mui/material"', '"element-plus"', '"lodash"', '"moment"', '"axios"', '"electron"'];
  for (const pkg of forbiddenNpm) {
    if (pkgRaw.includes(pkg)) {
      console.error(`  [FAIL] Forbidden heavy npm package ${pkg} in frontend/package.json`);
      feViolations++;
    }
  }
}
const bundleBudgets = {
  "frontend/dist/settings.html": 300 * 1024,
  "plugins/official-translate/ui/index.html": 150 * 1024,
  "plugins/official-clipboard/ui/index.html": 150 * 1024,
  "plugins/builtin-ocr/ui/index.html": 150 * 1024,
};
const bundleSummary = [];
for (const [bundlePath, limit] of Object.entries(bundleBudgets)) {
  if (!fs.existsSync(bundlePath)) continue;
  const bytes = fs.statSync(bundlePath).size;
  const kb = (bytes / 1024).toFixed(1);
  const limKb = Math.round(limit / 1024);
  if (bytes > limit) {
    console.error(`  [FAIL] ${bundlePath} is ${kb} KB (limit: ${limKb} KB)`);
    feViolations++;
  } else {
    bundleSummary.push(`${path.basename(bundlePath)}=${kb}KB/${limKb}KB`);
  }
}
violations += feViolations;
if (feViolations === 0) {
  console.log(`  [OK] Zero forbidden npm deps; bundles within budget (${bundleSummary.join(", ")}).`);
}

console.log(`=== [5/${totalSteps}] Checking Rust <-> TypeScript WebView IPC contract parity ===`);
let ipcViolations = 0;
const bridgePath = "crates/tomoyo-webview-host/src/bridge.rs";
const bridgeRaw = fs.readFileSync(bridgePath, "utf8");
const enumMatch = bridgeRaw.match(/pub enum HostMessage\s*\{([\s\S]*?)\r?\n\}/);
if (!enumMatch) {
  console.error(`  [FAIL] Could not locate pub enum HostMessage in ${bridgePath}`);
  ipcViolations++;
} else {
  const validCmds = new Set();
  for (const line of enumMatch[1].split(/\r?\n/)) {
    const m = line.match(/^\s+([A-Z][A-Za-z0-9]*)\b/);
    if (m) {
      validCmds.add(m[1].replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase());
    }
  }
  const feFiles = walk("frontend/src", (f) => f.endsWith(".ts") || f.endsWith(".svelte"));
  const checkedCmds = new Set();
  for (const ff of feFiles) {
    const src = fs.readFileSync(ff, "utf8");
    for (const cm of src.matchAll(/postHost\s*\(\s*\{\s*cmd\s*:\s*['"]([a-z0-9_]+)['"]/g)) {
      const cmd = cm[1];
      checkedCmds.add(cmd);
      if (!validCmds.has(cmd)) {
        console.error(`  [FAIL] Frontend sends unknown IPC cmd '${cmd}' in ${ff} (missing from HostMessage)`);
        ipcViolations++;
      }
    }
  }
  if (ipcViolations === 0) {
    console.log(`  [OK] Frontend -> Rust: all ${checkedCmds.size} IPC commands match Rust HostMessage (${validCmds.size} variants).`);
  }

  const rustEvents = new Set();
  for (const rf of walk("crates/tomoyo-webview-host/src", (f) => f.endsWith(".rs"))) {
    const rsrc = fs.readFileSync(rf, "utf8");
    for (const em of rsrc.matchAll(/"event"\s*:\s*"([a-z0-9_]+)"/g)) {
      rustEvents.add(em[1]);
    }
  }
  const feEvents = new Set();
  for (const ff of feFiles) {
    const src = fs.readFileSync(ff, "utf8");
    for (const em of src.matchAll(/(?:\bev\b|data\.event)\s*[!=]==\s*['"]([a-z0-9_]+)['"]/g)) {
      const evName = em[1];
      feEvents.add(evName);
      if (!rustEvents.has(evName)) {
        console.error(`  [FAIL] Frontend listens for unknown Rust HostEvent '${evName}' in ${ff}`);
        ipcViolations++;
      }
    }
  }
  for (const reName of rustEvents) {
    if (!feEvents.has(reName)) {
      console.error(`  [FAIL] Rust emits unhandled HostEvent '${reName}' (no frontend listener found)`);
      ipcViolations++;
    }
  }
  if (ipcViolations === 0) {
    console.log(`  [OK] Rust -> Frontend: symmetric 1:1 HostEvent parity (${feEvents.size}/${rustEvents.size} events matched).`);
  }
}
violations += ipcViolations;

console.log(`=== [6/${totalSteps}] Executing WebView UI JS runtime smoke gate ===`);
try {
  execFileSync(process.execPath, ["scripts/check_webview_ui.mjs"], { stdio: "inherit" });
} catch {
  console.error("  [FAIL] WebView UI runtime JS smoke check failed!");
  violations++;
}

if (checkBinaries) {
  console.log(`=== [7/${totalSteps}] Checking release binary size ceilings ===`);
  const isWin = process.platform === "win32";
  const ext = isWin ? ".exe" : "";
  const binBudgets = {
    [`target/release/tomoyo${ext}`]: isWin ? 5.8 : 6.2,
    [`target/release/tomoyo-webview-host${ext}`]: 4.2,
    [`target/release/tomoyo-capture${ext}`]: 1.6,
    [`target/release/tomoyo-plugin-ocr${ext}`]: 0.8,
    [`target/release/tomoyo-plugin-notes${ext}`]: 0.8,
    [`target/release/tomoyo-plugin-md-editor${ext}`]: 0.8,
    [`target/release/tomoyo-plugin-translate${ext}`]: 0.8,
    [`target/release/tomoyo-plugin-capture${ext}`]: 0.8,
    [`target/release/tomoyo-plugin-clipboard${ext}`]: 2.5,
  };
  let binViolations = 0;
  const binSummary = [];
  for (const [binPath, limitMb] of Object.entries(binBudgets)) {
    if (!fs.existsSync(binPath)) {
      console.error(`  [FAIL] Missing release binary: ${binPath}`);
      binViolations++;
      continue;
    }
    const mb = fs.statSync(binPath).size / (1024 * 1024);
    if (mb > limitMb) {
      console.error(`  [FAIL] ${binPath} is ${mb.toFixed(2)} MB (limit: ${limitMb} MB)`);
      binViolations++;
    } else {
      binSummary.push(`${path.basename(binPath)}=${mb.toFixed(2)}MB/${limitMb}MB`);
    }
  }
  violations += binViolations;
  if (binViolations === 0) {
    console.log(`  [OK] All release binaries within size budget (${binSummary.join(", ")}).`);
  }
}

if (violations > 0) {
  console.error(`\n[FAILED] Hygiene gate failed with ${violations} violation(s). See docs/CODING_STANDARDS.md.`);
  process.exit(1);
}
console.log("\n[PASS] All Tomoyo coding standards & anti-bloat checks passed!");
