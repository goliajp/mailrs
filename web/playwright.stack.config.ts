/**
 * Layout and live-update checks against a real local stack
 * (web/tests/stack-*.spec.ts). `PW_CONFIG=playwright.stack.config.ts
 * scripts/interaction-latency.sh` starts kevy-server, fastcore, webapi and
 * `vite preview` of the production build, then runs these instead of the
 * latency measurement; LATENCY_BASE is where the preview listens.
 */

import { defineConfig } from '@playwright/test'

export default defineConfig({
  expect: { timeout: 15_000 },
  fullyParallel: false,
  reporter: [['list']],
  retries: 0,
  testDir: './tests',
  testMatch: /stack-.*\.spec\.ts/,
  timeout: 120_000,
  use: {
    baseURL: process.env.LATENCY_BASE,
    headless: true,
    trace: 'off',
    video: 'off',
  },
  workers: 1,
})
