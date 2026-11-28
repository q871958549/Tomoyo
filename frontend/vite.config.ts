import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { viteSingleFile } from 'vite-plugin-singlefile';

const entry = process.env.TOMOYO_ENTRY || 'settings';

export default defineConfig({
  plugins: [svelte(), viteSingleFile()],
  esbuild: {
    legalComments: 'none',
    drop: ['debugger'],
  },
  build: {
    target: 'es2022',
    outDir: 'dist',
    emptyOutDir: false,
    reportCompressedSize: false,
    assetsInlineLimit: 100_000_000,
    cssCodeSplit: false,
    minify: 'esbuild',
    rollupOptions: {
      input: `${entry}.html`,
      output: {
        format: 'iife',
      },
    },
  },
});
