/**
 * Interaction latency (web/tests/interaction-latency.spec.ts). Its own
 * config because it needs no web servers of its own:
 * scripts/interaction-latency.sh starts the backend and `vite preview`
 * and passes LATENCY_BASE. The default config would start three servers
 * that sit idle on the machine during the measurement.
 */

import { defineConfig } from '@playwright/test'

export default defineConfig({
  fullyParallel: false,
  reporter: [['list']],
  retries: 0,
  testDir: './tests',
  testMatch: /interaction-latency\.spec\.ts/,
  timeout: 15 * 60_000,
  use: {
    baseURL: process.env.LATENCY_BASE,
    headless: true,
    trace: 'off',
    video: 'off',
  },
  workers: 1,
})
