import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    cssHash: ({ hash, css }) => `t-${hash(css).slice(0, 5)}`,
  },
};
