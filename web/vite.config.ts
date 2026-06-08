import { resolve } from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    coverage: {
      exclude: [
        'dist/**',
        'public/**',
        'src/**/__tests__/**',
        'src/**/*.test.*',
        'src/main.tsx',
        '*.config.*',
      ],
      provider: 'v8',
      reporter: ['text', 'text-summary'],
    },
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
  },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': resolve(import.meta.dirname, 'src') },
  },
  server: {
    proxy: {
      '/api': {
        changeOrigin: true,
        target: 'http://localhost:3200',
      },
    },
  },
})
