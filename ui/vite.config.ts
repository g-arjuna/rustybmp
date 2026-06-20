import { sveltekit } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [sveltekit()],
  server: {
    proxy: {
      '/api':     { target: 'http://localhost:7878', changeOrigin: true },
      '/health':  { target: 'http://localhost:7878', changeOrigin: true },
      '/metrics': { target: 'http://localhost:7878', changeOrigin: true },
    },
  },
});
