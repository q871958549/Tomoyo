#!/usr/bin/env node
// Deterministic Release Manifest Synchronizer for Scoop, Winget, and Arch/AUR PKGBUILD.
// Usage:
//   node scripts/sync-release-manifests.mjs --version 3.0.0 \
//     [--setup-sha256 <hex>] [--zip-sha256 <hex>] [--src-sha256 <hex>] \
//     [--out-dir dist] [--in-place] [--release-date YYYY-MM-DD]

import fs from 'node:fs';
import path from 'node:path';

function argVal(flag, fallback = '') {
  const i = process.argv.indexOf(flag);
  return i >= 0 && i + 1 < process.argv.length ? process.argv[i + 1] : fallback;
}

const version = argVal('--version').replace(/^v/i, '');
if (!version || !/^\d+\.\d+\.\d+/.test(version)) {
  console.error('Usage: node scripts/sync-release-manifests.mjs --version X.Y.Z [--setup-sha256 HEX] [--zip-sha256 HEX] [--src-sha256 HEX] [--out-dir dist] [--in-place]');
  process.exit(1);
}

const setupSha = argVal('--setup-sha256').toUpperCase();
const zipSha = argVal('--zip-sha256').toLowerCase();
const srcSha = argVal('--src-sha256').toLowerCase();
const outDir = argVal('--out-dir');
const inPlace = process.argv.includes('--in-place');
const releaseDate = argVal('--release-date', new Date().toISOString().slice(0, 10));

function writeTarget(relSrc, outName, content, crlf = false) {
  const norm = crlf
    ? content.replace(/\r\n/g, '\n').replace(/\n/g, '\r\n')
    : content.replace(/\r\n/g, '\n');
  if (outDir) {
    fs.mkdirSync(outDir, { recursive: true });
    fs.writeFileSync(path.join(outDir, outName), norm, 'utf8');
  }
  if (inPlace) {
    fs.writeFileSync(relSrc, norm, 'utf8');
  }
}

// 1. Scoop manifest
const scoopSrc = 'packaging/scoop/tomoyo.json';
if (fs.existsSync(scoopSrc)) {
  let s = fs.readFileSync(scoopSrc, 'utf8');
  s = s.replace(/"version":\s*"[^"]+"/, `"version": "${version}"`);
  s = s.replace(/\/releases\/download\/v[^/]+\/Tomoyo-v[^-]+-/g, `/releases/download/v${version}/Tomoyo-v${version}-`);
  if (zipSha) {
    s = s.replace(/"hash":\s*"[^"]+"/, `"hash": "${zipSha}"`);
  }
  writeTarget(scoopSrc, 'tomoyo.scoop.json', s, false);
}

// 2. Winget manifests (UTF-8 without BOM, CRLF)
const wingetFiles = [
  'TomoyoContributors.Tomoyo.yaml',
  'TomoyoContributors.Tomoyo.installer.yaml',
  'TomoyoContributors.Tomoyo.locale.en-US.yaml',
  'TomoyoContributors.Tomoyo.locale.zh-CN.yaml',
];
for (const wf of wingetFiles) {
  const src = path.join('packaging/winget', wf);
  if (!fs.existsSync(src)) continue;
  let w = fs.readFileSync(src, 'utf8');
  w = w.replace(/^PackageVersion:\s*.+$/gm, `PackageVersion: ${version}`);
  w = w.replace(/^ReleaseDate:\s*.+$/gm, `ReleaseDate: ${releaseDate}`);
  w = w.replace(/^(\s*-\s*DisplayName:\s*Tomoyo v).+$/gm, `$1${version}`);
  w = w.replace(/\/releases\/download\/v[^/]+\/Tomoyo-v[^-]+-/g, `/releases/download/v${version}/Tomoyo-v${version}-`);
  w = w.replace(/\/releases\/tag\/v[^\s]+/g, `/releases/tag/v${version}`);
  w = w.replace(/\/blob\/v[^/]+\/LICENSE-MIT/g, `/blob/v${version}/LICENSE-MIT`);
  if (setupSha) {
    w = w.replace(/^(\s*InstallerSha256:\s*).+$/gm, `$1${setupSha}`);
  }
  writeTarget(src, wf, w, true);
}

// 3. Arch Linux / AUR PKGBUILDs & .SRCINFO
const pkgbuildSrc = 'packaging/linux/PKGBUILD';
if (fs.existsSync(pkgbuildSrc)) {
  let p = fs.readFileSync(pkgbuildSrc, 'utf8');
  p = p.replace(/^pkgver=.+$/m, `pkgver=${version}`);
  p = p.replace(/^pkgrel=.+$/m, 'pkgrel=1');
  if (srcSha) {
    p = p.replace(/^sha256sums=\('[^']+'\)/m, `sha256sums=('${srcSha}')`);
  }
  writeTarget(pkgbuildSrc, 'PKGBUILD', p, false);
}

const aurPkgbuildSrc = 'packaging/linux/aur-tomoyo-launcher/PKGBUILD';
if (fs.existsSync(aurPkgbuildSrc)) {
  let p = fs.readFileSync(aurPkgbuildSrc, 'utf8');
  p = p.replace(/^pkgver=.+$/m, `pkgver=${version}`);
  p = p.replace(/^pkgrel=.+$/m, 'pkgrel=1');
  if (srcSha) {
    p = p.replace(/^sha256sums=\('[^']+'\)/m, `sha256sums=('${srcSha}')`);
  }
  writeTarget(aurPkgbuildSrc, 'PKGBUILD.aur', p, false);
}

const aurSrcinfo = 'packaging/linux/aur-tomoyo-launcher/.SRCINFO';
if (fs.existsSync(aurSrcinfo)) {
  let s = fs.readFileSync(aurSrcinfo, 'utf8');
  s = s.replace(/^(\tpkgver = ).+$/m, `$1${version}`);
  s = s.replace(/^(\tpkgrel = ).+$/m, '$11');
  s = s.replace(/^(\tsource = )Tomoyo-[^.]+\.[^.]+\.[^.]+\.tar\.gz::(.+)\/refs\/tags\/v[^\n]+$/m, `$1Tomoyo-${version}.tar.gz::$2/refs/tags/v${version}.tar.gz`);
  if (srcSha) {
    s = s.replace(/^(\tsha256sums = ).+$/m, `$1${srcSha}`);
  }
  writeTarget(aurSrcinfo, '.SRCINFO.aur', s, false);
}

console.log(`[OK] Synced release manifests for v${version} (date=${releaseDate}, outDir=${outDir || '-'}, inPlace=${inPlace})`);
