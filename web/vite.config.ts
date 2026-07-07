import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

// Cache-buster for the React Query persister key. Stamped at config-load
// time, so every `vite build` (and every `vite dev` restart) emits a new
// value — guarantees a localStorage rotation on each release. Without
// this, query-client.ts's `__APP_BUILD_ID__` was always undefined →
// persister key never changed → `staleTime: Infinity` queries
// (e.g. useThreadQuery) kept serving the pre-deploy JSON forever and
// new wire fields silently never reached the UI.
const BUILD_ID = `${Date.now()}`

// Webapp version — read from package.json at config-load time.
// release-web.yml `sed`s the tag date into package.json before
// running `vite build`, so this pulls the actual shipping version
// into the bundle. StatusBar (see app.tsx) surfaces it beside the
// backend version so an operator can spot a stale bundle.
const PACKAGE_VERSION = (() => {
  try {
    const pkg = JSON.parse(
      readFileSync(resolve(import.meta.dirname, 'package.json'), 'utf8')
    )
    return String(pkg.version ?? 'dev')
  } catch {
    return 'dev'
  }
})()

export default defineConfig({
  define: {
    __APP_BUILD_ID__: JSON.stringify(BUILD_ID),
    __WEB_VERSION__: JSON.stringify(PACKAGE_VERSION),
  },
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
