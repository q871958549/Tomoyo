#!/usr/bin/env node
// Release-readiness & distribution audit for Tomoyo v4.0.0+.
// Verifies SDK contracts, JSON/YAML/AUR manifests, documentation links, and dry-run manifest sync.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

function assertExists(rel) {
  if (!fs.existsSync(rel)) {
    console.error(`[FAIL] Missing required release artifact/doc: ${rel}`);
    process.exit(1);
  }
}

function readJson(rel) {
  assertExists(rel);
  try {
    return JSON.parse(fs.readFileSync(rel, 'utf8'));
  } catch (err) {
    console.error(`[FAIL] Invalid JSON in ${rel}: ${err.message}`);
    process.exit(1);
  }
}

const requiredFiles = [
  'docs/PROTOCOL_STABILITY.md',
  'docs/RELEASE_SIGNING.md',
  'docs/DISTRIBUTION.md',
  'docs/WAYLAND_PORTAL_GLOBALSHORTCUTS.md',
  '.github/workflows/publish-sdk.yml',
  '.github/workflows/release.yml',
  'packaging/linux/PKGBUILD',
  'packaging/linux/aur-tomoyo-launcher/PKGBUILD',
  'packaging/linux/aur-tomoyo-launcher/.SRCINFO',
  'packaging/winget/TomoyoContributors.Tomoyo.yaml',
  'packaging/winget/TomoyoContributors.Tomoyo.installer.yaml',
  'packaging/winget/TomoyoContributors.Tomoyo.locale.en-US.yaml',
  'packaging/winget/TomoyoContributors.Tomoyo.locale.zh-CN.yaml',
];
requiredFiles.forEach(assertExists);

const scoop = readJson('packaging/scoop/tomoyo.json');
if (!scoop.architecture?.['64bit']?.url || !scoop.architecture?.arm64?.url) {
  console.error('[FAIL] packaging/scoop/tomoyo.json must define both 64bit and arm64 architectures');
  process.exit(1);
}

const regIndex = readJson('registry/index.json');
if (!Array.isArray(regIndex.shards) || regIndex.shards.length === 0) {
  console.error('[FAIL] registry/index.json must declare non-empty shards array');
  process.exit(1);
}
let totalShardPlugins = 0;
for (const entry of regIndex.shards) {
  const shardJson = readJson(entry.source);
  const count = Array.isArray(shardJson.plugins) ? shardJson.plugins.length : 0;
  totalShardPlugins += count;
}
if (totalShardPlugins < 14 || (regIndex.total_count ?? 0) !== totalShardPlugins) {
  console.error(`[FAIL] registry shard count mismatch (total_count=${regIndex.total_count}, shards=${totalShardPlugins})`);
  process.exit(1);
}

const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tomoyo-rel-audit-'));
try {
  execFileSync(
    process.execPath,
    [
      'scripts/sync-release-manifests.mjs',
      '--version',
      '4.0.0',
      '--setup-sha256',
      'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      '--zip-sha256',
      'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      '--arm64-zip-sha256',
      'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
      '--src-sha256',
      'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
      '--out-dir',
      tmpDir,
    ],
    { stdio: 'pipe' },
  );

  const expectedOut = [
    'tomoyo.scoop.json',
    'TomoyoContributors.Tomoyo.yaml',
    'TomoyoContributors.Tomoyo.installer.yaml',
    'TomoyoContributors.Tomoyo.locale.en-US.yaml',
    'TomoyoContributors.Tomoyo.locale.zh-CN.yaml',
    'PKGBUILD',
    'PKGBUILD.aur',
    '.SRCINFO.aur',
  ];
  for (const name of expectedOut) {
    const p = path.join(tmpDir, name);
    assertExists(p);
    const body = fs.readFileSync(p, 'utf8');
    if (!body.includes('4.0.0')) {
      console.error(`[FAIL] Synced manifest ${name} did not update version to 4.0.0`);
      process.exit(1);
    }
  }
  const syncedScoop = JSON.parse(fs.readFileSync(path.join(tmpDir, 'tomoyo.scoop.json'), 'utf8'));
  if (
    syncedScoop.architecture['64bit'].hash !== 'b'.repeat(64) ||
    syncedScoop.architecture.arm64.hash !== 'c'.repeat(64)
  ) {
    console.error('[FAIL] Synced scoop manifest did not assign distinct 64bit and arm64 SHA-256 hashes');
    process.exit(1);
  }
} finally {
  fs.rmSync(tmpDir, { recursive: true, force: true });
}

console.log(`[OK] Release readiness verified (${requiredFiles.length} docs/manifests + ${totalShardPlugins} registry plugins across ${regIndex.shards.length} shards + dry-run sync).`);
