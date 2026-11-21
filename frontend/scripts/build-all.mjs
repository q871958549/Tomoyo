// Builds all 4 ephemeral WebView2 single-file HTML bundles into frontend/dist/.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const distDir = path.join(root, 'dist');
fs.rmSync(distDir, { recursive: true, force: true });
fs.mkdirSync(distDir, { recursive: true });

const viteBin = path.join(root, 'node_modules', 'vite', 'bin', 'vite.js');
const entries = ['settings', 'translator', 'clipboard', 'ocr'];

for (const entry of entries) {
  execFileSync(process.execPath, [viteBin, 'build'], {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env, TOMOYO_ENTRY: entry },
  });
  // Relocate inlined IIFE script to end of <body> as classic <script> for zero-CORS WebView2 NavigateToString safety.
  // Note: uses index slicing + function replacer so literal `$` sequences in minified JS are never mangled.
  const outFile = path.join(distDir, `${entry}.html`);
  let html = fs.readFileSync(outFile, 'utf8');
  const openIdx = html.search(/<script\b[^>]*>/i);
  const closeIdx = html.lastIndexOf('</script>');
  if (openIdx >= 0 && closeIdx > openIdx) {
    const openEnd = html.indexOf('>', openIdx) + 1;
    const js = html.slice(openEnd, closeIdx);
    const withoutScript = html.slice(0, openIdx) + html.slice(closeIdx + '</script>'.length);
    html = withoutScript.replace('</body>', () => `<script>${js}</script>\n</body>`);
    fs.writeFileSync(outFile, html, 'utf8');
  }
}

const summary = entries.map((e) => {
  const sz = fs.statSync(path.join(distDir, `${e}.html`)).size;
  return `${e}.html (${(sz / 1024).toFixed(1)} KB)`;
});
console.log(`[OK] Built 4 single-file WebView bundles: ${summary.join(' · ')}`);
