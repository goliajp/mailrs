import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import type { Plugin } from 'vite'
import { defineConfig } from 'vitest/config'

function swCacheBump(): Plugin {
  return {
    name: 'sw-cache-bump',
    apply: 'build',
    closeBundle() {
      const pkg = JSON.parse(
        readFileSync(resolve(import.meta.dirname, 'package.json'), 'utf-8'),
      ) as { version: string }
      const swPath = resolve(import.meta.dirname, 'dist/sw.js')
      const src = readFileSync(swPath, 'utf-8')
      const next = src.replace(
        /const CACHE_NAME = '[^']*'/,
        `const CACHE_NAME = 'mailrs-v${pkg.version}'`,
      )
      if (next === src) {
        throw new Error('sw-cache-bump: CACHE_NAME line not found in dist/sw.js')
      }
      writeFileSync(swPath, next)
    },
  }
}

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
  plugins: [react(), tailwindcss(), swCacheBump()],
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
