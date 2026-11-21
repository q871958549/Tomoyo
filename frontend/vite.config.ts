import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { viteSingleFile } from 'vite-plugin-singlefile';

const entry = process.env.TOMOYO_ENTRY || 'settings';

export default defineConfig({
  plugins: [svelte(), tailwindcss(), viteSingleFile()],
  build: {
    outDir: 'dist',
    emptyOutDir: false,
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
