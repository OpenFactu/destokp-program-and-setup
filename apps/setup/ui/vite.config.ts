import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Tauri sirve la interfaz desde este servidor en desarrollo y desde `dist` en
// el binario final. El puerto es fijo porque `tauri.conf.json` apunta a él.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    // El WebView2 de Windows 11 va muy por delante de esto; el objetivo es
    // sólo evitar sorpresas en equipos con versiones antiguas.
    target: 'chrome110',
    sourcemap: false,
  },
});
