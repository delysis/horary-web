import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { execFileSync } from 'node:child_process'
import { rmSync } from 'node:fs'

export default defineConfig({
  plugins: [react(), { name: 'native-assets', closeBundle() { if (process.env.TAURI_ENV_PLATFORM) rmSync('dist/litert', { recursive: true, force: true }) } }],
  define: { __HORARY_BUILD__: JSON.stringify(execFileSync('git', ['describe', '--always', '--dirty'], { encoding: 'utf8' }).trim()) },
  base: process.env.TAURI_ENV_PLATFORM ? './' : '/horary-web/',
  test: {
    environment: 'jsdom',
    include: ['src/__tests__/**/*.test.ts'],
  },
})
