import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  base: '/horary-web/',
  test: {
    environment: 'jsdom',
    include: ['src/__tests__/**/*.test.ts'],
  },
})
