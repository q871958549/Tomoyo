#!/usr/bin/env node
// Activates versioned Git anti-bloat hooks (.githooks) and optional weekly systemd --user workspace GC timer.
// Usage: node scripts/setup-governance.mjs [--install-timer]

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";

const repoRoot = path.resolve(".");
const installTimer = process.argv.includes("--install-timer");

for (const hook of [".githooks/pre-commit", ".githooks/pre-push"]) {
  if (!fs.existsSync(hook)) {
    console.error(`[FAIL] Missing hook script: ${hook}`);
    process.exit(1);
  }
  if (process.platform !== "win32") {
    fs.chmodSync(hook, 0o755);
  }
}

execFileSync("git", ["config", "core.hooksPath", ".githooks"], { stdio: "inherit" });
const configured = execFileSync("git", ["config", "--get", "core.hooksPath"], { encoding: "utf8" }).trim();
console.log(`[OK] Git hooks enabled: core.hooksPath=${configured}`);

if (installTimer) {
  if (process.platform !== "linux") {
    console.log("[INFO] --install-timer is Linux systemd --user specific; run `node scripts/workspace-gc.mjs --apply` via Task Scheduler or manually on this OS.");
    process.exit(0);
  }
  const hasSystemctl = spawnSync("systemctl", ["--user", "status"], { stdio: "ignore" }).status !== null;
  if (!hasSystemctl) {
    console.warn("[WARN] systemd --user not available in this environment; skipping timer installation.");
    process.exit(0);
  }

  const unitDir = path.join(os.homedir(), ".config", "systemd", "user");
  fs.mkdirSync(unitDir, { recursive: true });
  const nodeBin = process.execPath;
  const gcScript = path.join(repoRoot, "scripts", "workspace-gc.mjs");

  const serviceUnit = `[Unit]
Description=Tomoyo Workspace Anti-Bloat Incremental Cache GC

[Service]
Type=oneshot
WorkingDirectory=${repoRoot}
Nice=19
IOSchedulingClass=idle
ExecStart=${nodeBin} ${gcScript} --apply --max-age-days=7
`;
  const timerUnit = `[Unit]
Description=Weekly Tomoyo Workspace Disk GC

[Timer]
OnCalendar=weekly
Persistent=true
RandomizedDelaySec=1h

[Install]
WantedBy=timers.target
`;

  fs.writeFileSync(path.join(unitDir, "tomoyo-workspace-gc.service"), serviceUnit, "utf8");
  fs.writeFileSync(path.join(unitDir, "tomoyo-workspace-gc.timer"), timerUnit, "utf8");

  execFileSync("systemctl", ["--user", "daemon-reload"], { stdio: "inherit" });
  execFileSync("systemctl", ["--user", "enable", "--now", "tomoyo-workspace-gc.timer"], { stdio: "inherit" });
  const active = execFileSync("systemctl", ["--user", "is-active", "tomoyo-workspace-gc.timer"], {
    encoding: "utf8",
  }).trim();
  console.log(`[OK] Installed & enabled tomoyo-workspace-gc.timer (state=${active})`);
}
