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

// Webapp version — decides what the bottom StatusBar prints as
// `web <version>`. Sources, in priority order:
//   1. `WEB_VERSION` env var — set by release-web.yml so a shipped
//      bundle carries the tag (`2026.07.07-1`) that produced it.
//   2. `GITHUB_REF_NAME` env var stripped of the `web-v` prefix, so
//      any workflow that already exports GITHUB_REF_NAME works.
//   3. package.json `version` — the historical fallback; permanently
//      pinned at "0.0.0" per repo convention, so this is only useful
//      for a local one-off build.
// A resolved version of "0.0.0" is treated as "dev" downstream (see
// app.tsx), so a stale package.json placeholder never leaks into the
// UI as if it were a real version.
const WEB_VERSION = (() => {
  const raw = process.env.WEB_VERSION ?? stripTagPrefix(process.env.GITHUB_REF_NAME)
  if (raw && raw.length > 0) return raw
  try {
    const pkg = JSON.parse(readFileSync(resolve(import.meta.dirname, 'package.json'), 'utf8'))
    return String(pkg.version ?? 'dev')
  } catch {
    return 'dev'
  }
})()

function stripTagPrefix(ref: string | undefined): string | undefined {
  if (!ref) return undefined
  return ref.replace(/^web-v/, '').replace(/^v/, '')
}

export default defineConfig({
  define: {
    __APP_BUILD_ID__: JSON.stringify(BUILD_ID),
    __WEB_VERSION__: JSON.stringify(WEB_VERSION),
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
