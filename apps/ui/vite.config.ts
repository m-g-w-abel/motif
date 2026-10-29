import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// In a browser dev session, /ws is proxied to a headless engine
// (`cargo run -p headless`). Inside Tauri the UI connects to the embedded
// engine directly, so the proxy is unused there.
export default defineConfig(({ command }) => ({
  plugins: [
    svelte({
      // In development, inject component CSS from its JavaScript instead of
      // separate virtual CSS modules: webviews (notably WebKitGTK) that reuse
      // cached modules across dev-server restarts otherwise lose styles.
      // Production builds still emit a single CSS file.
      emitCss: command === 'build',
    }),
  ],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/ws': { target: 'ws://127.0.0.1:7700', ws: true },
      '/art': 'http://127.0.0.1:7700',
      '/images': 'http://127.0.0.1:7700',
    },
  },
  build: {
    target: 'es2022',
  },
}));
