/**
 * Playwright config — v2.7.3 §Phase 12 §12.8 webapp perf baseline.
 *
 * Two tiers:
 *   1. Unauthenticated tier (CI-safe): `vite preview` serves the
 *      production build locally; specs measure login-page LCP / CLS
 *      against fixed budgets. Fully deterministic — no network, no
 *      credentials, no staging dependency.
 *   2. Authenticated tier (opt-in): when `MAILRS_E2E_BASE` +
 *      `MAILRS_E2E_USER` + `MAILRS_E2E_PASS` are set, the full
 *      3-flow suite (dashboard load / open thread / compose reply)
 *      runs against that live instance. CI leaves these unset so
 *      the tier self-skips; run locally against staging for the
 *      complete picture.
 *
 * Run: `bun run test:perf` (builds first via the script chain).
 */

import { defineConfig } from '@playwright/test'

const useExternal = !!process.env.MAILRS_E2E_BASE

export default defineConfig({
  expect: { timeout: 10_000 },
  fullyParallel: false,
  reporter: [['list'], ['json', { outputFile: 'perf-results/report.json' }]],
  retries: 0,
  testDir: './tests',
  timeout: 60_000,
  use: {
    // `localhost` not 127.0.0.1 — vite preview binds the IPv6
    // loopback on macOS and the IPv4 literal never connects.
    baseURL: process.env.MAILRS_E2E_BASE ?? 'http://localhost:4173',
    trace: 'off',
    video: 'off',
  },
  webServer: useExternal
    ? undefined
    : {
        command: 'bunx vite preview --port 4173 --strictPort',
        reuseExistingServer: true,
        timeout: 30_000,
        url: 'http://localhost:4173',
      },
  workers: 1,
})
