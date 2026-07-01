import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';
import { VitePWA } from 'vite-plugin-pwa';
import { createPwaManifest, normalizePwaBase } from './src/pwa/pwaManifest.js';

const packageJson = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
const isTauri = process.env.TAURI_ENV_PLATFORM != null;
const webBase = normalizePwaBase(process.env.VITE_BASE_PATH || '/');
const base = isTauri ? './' : webBase;

export default defineConfig({
  base,
  define: {
    __WHORARY_APP_VERSION__: JSON.stringify(packageJson.version)
  },
  plugins: [
    !isTauri && VitePWA({
      registerType: 'autoUpdate',
      includeAssets: ['favicon.svg', 'apple-touch-icon.png'],
      manifest: createPwaManifest(webBase),
      workbox: {
        globPatterns: ['**/*.{js,css,html,svg,png,woff2}']
      }
    })
  ].filter(Boolean),
  build: {
    target: 'es2020',
    outDir: 'dist'
  }
});
