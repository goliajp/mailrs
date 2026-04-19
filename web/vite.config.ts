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
        'src/vite-env.d.ts',
        '*.config.*',
      ],
      provider: 'v8',
      reporter: ['text', 'text-summary'],
      thresholds: {
        'src/hooks/**': {
          branches: 65,
          functions: 65,
          lines: 35,
          statements: 35,
        },
        'src/lib/**': {
          branches: 80,
          functions: 80,
          lines: 80,
          statements: 80,
        },
        'src/store/**': {
          branches: 80,
          functions: 80,
          lines: 80,
          statements: 80,
        },
      },
    },
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
  },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': resolve(import.meta.dirname, 'src') },
  },
  // no manualChunks — rolldown's automatic chunking respects dynamic imports.
  // (the previous editor/markdown manual groups dragged shared deps like jotai
  //  into the tiptap chunk, which then leaked back into the entry preload.)
  server: {
    proxy: {
      '/api': {
        changeOrigin: true,
        target: 'http://localhost:3200',
      },
    },
  },
})
