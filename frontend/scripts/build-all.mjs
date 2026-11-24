// Builds ephemeral WebView single-file HTML bundles (core Settings + Hybrid official plugin UIs).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(root, '..');
const distDir = path.join(root, 'dist');
fs.rmSync(distDir, { recursive: true, force: true });
fs.mkdirSync(distDir, { recursive: true });

const viteBin = path.join(root, 'node_modules', 'vite', 'bin', 'vite.js');
const targets = [
  { entry: 'settings', outFile: path.join(distDir, 'settings.html'), label: 'settings.html' },
  { entry: 'translator', outFile: path.join(repoRoot, 'plugins', 'official-translate', 'ui', 'index.html'), label: 'official-translate/ui/index.html' },
  { entry: 'clipboard', outFile: path.join(distDir, 'clipboard.html'), label: 'clipboard.html' },
  { entry: 'ocr', outFile: path.join(repoRoot, 'plugins', 'builtin-ocr', 'ui', 'index.html'), label: 'builtin-ocr/ui/index.html' },
];

for (const { entry, outFile } of targets) {
  execFileSync(process.execPath, [viteBin, 'build'], {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env, TOMOYO_ENTRY: entry },
  });
  // Relocate inlined IIFE script to end of <body> as classic <script> for zero-CORS WebView safety.
  // Note: uses index slicing + function replacer so literal `$` sequences in minified JS are never mangled.
  const builtFile = path.join(distDir, `${entry}.html`);
  let html = fs.readFileSync(builtFile, 'utf8');
  const openIdx = html.search(/<script\b[^>]*>/i);
  const closeIdx = html.lastIndexOf('</script>');
  if (openIdx >= 0 && closeIdx > openIdx) {
    const openEnd = html.indexOf('>', openIdx) + 1;
    const js = html.slice(openEnd, closeIdx);
    const withoutScript = html.slice(0, openIdx) + html.slice(closeIdx + '</script>'.length);
    html = withoutScript.replace('</body>', () => `<script>${js}</script>\n</body>`);
  }
  fs.mkdirSync(path.dirname(outFile), { recursive: true });
  fs.writeFileSync(outFile, html, 'utf8');
  if (path.resolve(outFile) !== path.resolve(builtFile)) {
    fs.rmSync(builtFile, { force: true });
  }
}

const summary = targets.map(({ outFile, label }) => {
  const sz = fs.statSync(outFile).size;
  return `${label} (${(sz / 1024).toFixed(1)} KB)`;
});
console.log(`[OK] Built ${targets.length} single-file WebView bundles: ${summary.join(' · ')}`);
