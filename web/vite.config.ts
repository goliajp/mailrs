import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { visualizer } from 'rollup-plugin-visualizer'
import type { Plugin, PluginOption } from 'vite'
import { defineConfig } from 'vitest/config'

function buildPlugins(): PluginOption[] {
  const plugins: PluginOption[] = [react(), tailwindcss(), swCacheBump()]
  // Opt-in bundle analyzer: `BUNDLE_ANALYZE=1 vite build` writes
  // dist/stats.html with a treemap of every module's bundled size. Off
  // by default so normal builds stay fast.
  if (process.env.BUNDLE_ANALYZE) {
    plugins.push(
      visualizer({
        filename: 'dist/stats.html',
        gzipSize: true,
        template: 'treemap',
      }) as PluginOption
    )
  }
  return plugins
}

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

const pkg = JSON.parse(
  readFileSync(resolve(import.meta.dirname, 'package.json'), 'utf-8'),
) as { version: string }

export default defineConfig({
  // __MAILRS_VERSION__ is stamped at build time so runtime code (error
  // reporter, status bar) can ship its version without importing package.json
  // (which would need resolveJsonModule + drag the full JSON into the bundle).
  define: {
    __MAILRS_VERSION__: JSON.stringify(pkg.version),
  },
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
  plugins: buildPlugins(),
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
