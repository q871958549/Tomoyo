#!/usr/bin/env node
// Cross-platform workspace & runtime disk bloat auditor and safe garbage collector.
// Usage:
//   node scripts/workspace-gc.mjs [--dry-run] [--apply] [--max-age-days=7] [--prune-debug-all] [--json]

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const args = process.argv.slice(2);
const apply = args.includes("--apply");
const pruneDebugAll = args.includes("--prune-debug-all");
const jsonOut = args.includes("--json");
const maxAgeDays = Math.max(
  1,
  Number(args.find((a) => a.startsWith("--max-age-days="))?.split("=")[1] ?? 7),
);
const cutoffMs = Date.now() - maxAgeDays * 86_400_000;

function dirSize(dir) {
  if (!fs.existsSync(dir)) return 0;
  let total = 0;
  const stack = [dir];
  while (stack.length > 0) {
    const cur = stack.pop();
    let ents;
    try {
      ents = fs.readdirSync(cur, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of ents) {
      const full = path.join(cur, ent.name);
      if (ent.isSymbolicLink()) continue;
      if (ent.isDirectory()) {
        stack.push(full);
      } else {
        try {
          total += fs.statSync(full).size;
        } catch {
          // ignore transient files
        }
      }
    }
  }
  return total;
}

function sweepStaleInDir(dir, isDryRun) {
  if (!fs.existsSync(dir)) return 0;
  let reclaimed = 0;
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, ent.name);
    let st;
    try {
      st = fs.lstatSync(full);
    } catch {
      continue;
    }
    if (st.isSymbolicLink()) continue;
    if (ent.isDirectory()) {
      if (st.mtimeMs < cutoffMs) {
        const bytes = dirSize(full);
        reclaimed += bytes;
        if (!isDryRun) fs.rmSync(full, { recursive: true, force: true });
      } else {
        reclaimed += sweepStaleInDir(full, isDryRun);
      }
    } else if (st.mtimeMs < cutoffMs) {
      reclaimed += st.size;
      if (!isDryRun) fs.rmSync(full, { force: true });
    }
  }
  return reclaimed;
}

function userDataDir() {
  if (process.platform === "win32") {
    return path.join(process.env.LOCALAPPDATA || os.tmpdir(), "Tomoyo");
  }
  if (process.env.XDG_DATA_HOME?.trim()) {
    return path.join(process.env.XDG_DATA_HOME, "tomoyo");
  }
  return path.join(os.homedir(), ".local", "share", "tomoyo");
}

function fmtMb(bytes) {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 ** 3)).toFixed(2)} GB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

const targets = {
  targetDebug: path.resolve("target/debug"),
  targetRelease: path.resolve("target/release"),
  targetDoc: path.resolve("target/doc"),
  targetPackage: path.resolve("target/package"),
  nodeModules: path.resolve("frontend/node_modules"),
  userData: userDataDir(),
};

const before = {
  targetDebug: dirSize(targets.targetDebug),
  targetRelease: dirSize(targets.targetRelease),
  targetDoc: dirSize(targets.targetDoc),
  targetPackage: dirSize(targets.targetPackage),
  nodeModules: dirSize(targets.nodeModules),
  userData: dirSize(targets.userData),
  clipboardImages: dirSize(path.join(targets.userData, "clipboard-images")),
  logs: dirSize(path.join(targets.userData, "logs")),
};

let reclaimed = 0;
if (pruneDebugAll) {
  for (const p of [targets.targetDebug, targets.targetPackage]) {
    if (fs.existsSync(p)) {
      reclaimed += dirSize(p);
      if (apply) fs.rmSync(p, { recursive: true, force: true });
    }
  }
} else {
  const staleRoots = [
    path.join(targets.targetDebug, "deps"),
    path.join(targets.targetDebug, "incremental"),
    path.join(targets.targetDebug, "build"),
    path.join(targets.targetDebug, ".fingerprint"),
    targets.targetDoc,
    targets.targetPackage,
  ];
  for (const root of staleRoots) {
    reclaimed += sweepStaleInDir(root, !apply);
  }
}

const after = apply
  ? {
      targetDebug: dirSize(targets.targetDebug),
      targetRelease: dirSize(targets.targetRelease),
      targetDoc: dirSize(targets.targetDoc),
      targetPackage: dirSize(targets.targetPackage),
    }
  : {
      targetDebug: Math.max(0, before.targetDebug - reclaimed),
      targetRelease: before.targetRelease,
      targetDoc: before.targetDoc,
      targetPackage: before.targetPackage,
    };

if (jsonOut) {
  console.log(JSON.stringify({ mode: apply ? "apply" : "dry-run", maxAgeDays, pruneDebugAll, before, after, reclaimed }, null, 2));
} else {
  console.log(`=== Tomoyo Workspace GC (${apply ? "APPLIED" : "DRY-RUN"}, age>${maxAgeDays}d, pruneDebugAll=${pruneDebugAll}) ===`);
  console.log(`  target/debug      : ${fmtMb(before.targetDebug)} -> ${fmtMb(after.targetDebug)}`);
  console.log(`  target/release    : ${fmtMb(before.targetRelease)} (preserved)`);
  console.log(`  target/doc        : ${fmtMb(before.targetDoc)}`);
  console.log(`  target/package    : ${fmtMb(before.targetPackage)}`);
  console.log(`  frontend/node_mod : ${fmtMb(before.nodeModules)}`);
  console.log(`  runtime data_dir  : ${fmtMb(before.userData)} (clipboard-images=${fmtMb(before.clipboardImages)}, logs=${fmtMb(before.logs)})`);
  console.log(`  ${apply ? "Reclaimed" : "Reclaimable"}       : ${fmtMb(reclaimed)}`);
}
