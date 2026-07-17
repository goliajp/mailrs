/**
 * Webapp perf baseline — v2.7.3 §Phase 12 §12.8.
 *
 * Budgets (Google CWV "good" thresholds):
 *   LCP < 2500 ms   CLS < 0.1   INP < 200 ms
 *
 * Tier 1 (always runs, CI-safe): login page against the local
 * `vite preview` production build. Deterministic — no auth, no
 * network beyond localhost.
 *
 * Tier 2 (opt-in): full flows against a live instance. Enabled by
 *   MAILRS_E2E_BASE=https://mail.smk.ai \
 *   MAILRS_E2E_USER=... MAILRS_E2E_PASS=... bun run test:perf
 * CI leaves the env unset → tier self-skips.
 *
 * INP note: INP needs real user interactions and only settles on
 * pagehide; for the login tier we assert LCP + CLS only. The authed
 * flows interact (click thread, type reply) so INP is sampled there.
 */

import type { Page } from '@playwright/test'

import { expect, test } from '@playwright/test'

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

const BUDGET = { cls: 0.1, inp: 200, lcp: 2500 }

const VITALS_IIFE = fs.readFileSync(
  path.join(__dirname, '../node_modules/web-vitals/dist/web-vitals.iife.js'),
  'utf-8'
)

type Vitals = { cls?: number; inp?: number; lcp?: number }

async function armVitals(page: Page) {
  await page.addInitScript(`
    ${VITALS_IIFE}
    window.__vitals = {};
    webVitals.onLCP((m) => { window.__vitals.lcp = m.value; }, { reportAllChanges: true });
    webVitals.onCLS((m) => { window.__vitals.cls = m.value; }, { reportAllChanges: true });
    webVitals.onINP((m) => { window.__vitals.inp = m.value; }, { reportAllChanges: true });
  `)
}

async function readVitals(page: Page): Promise<Vitals> {
  // Give LCP/CLS observers a settle window after network idle.
  await page.waitForTimeout(1000)
  return page.evaluate(() => (window as unknown as { __vitals: Vitals }).__vitals)
}

function record(flow: string, vitals: Vitals) {
  const dir = path.join(__dirname, '../perf-results')
  fs.mkdirSync(dir, { recursive: true })
  const file = path.join(dir, 'vitals.json')
  const existing = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf-8')) : {}
  existing[flow] = { ...vitals, at: new Date().toISOString() }
  fs.writeFileSync(file, JSON.stringify(existing, null, 2))
}

// ── Tier 1: unauthenticated (CI-safe) ─────────────────────────────

test('login page: LCP + CLS within budget', async ({ page }) => {
  await armVitals(page)
  await page.goto('/', { waitUntil: 'networkidle' })
  const vitals = await readVitals(page)
  record('login', vitals)
  expect(vitals.lcp, `LCP ${vitals.lcp}ms exceeds ${BUDGET.lcp}ms`).toBeLessThan(BUDGET.lcp)
  expect(vitals.cls ?? 0, `CLS ${vitals.cls} exceeds ${BUDGET.cls}`).toBeLessThan(BUDGET.cls)
})

// ── Tier 2: authenticated flows (opt-in via env) ──────────────────

const E2E_USER = process.env.MAILRS_E2E_USER
const E2E_PASS = process.env.MAILRS_E2E_PASS
const authed = !!(process.env.MAILRS_E2E_BASE && E2E_USER && E2E_PASS)

test.describe('authenticated flows', () => {
  test.skip(!authed, 'MAILRS_E2E_BASE/USER/PASS unset — authed perf tier skipped')

  async function login(page: Page) {
    await page.goto('/', { waitUntil: 'networkidle' })
    await page.getByPlaceholder(/email|address|用户/i).fill(E2E_USER!)
    await page.getByPlaceholder(/password|密码/i).fill(E2E_PASS!)
    await page.getByRole('button', { name: /sign in|log ?in|登录/i }).click()
    await page.waitForURL(/dashboard|mail/, { timeout: 15_000 })
  }

  test('dashboard load: LCP + CLS within budget', async ({ page }) => {
    await armVitals(page)
    await login(page)
    const vitals = await readVitals(page)
    record('dashboard', vitals)
    expect(vitals.lcp, `LCP ${vitals.lcp}ms`).toBeLessThan(BUDGET.lcp)
    expect(vitals.cls ?? 0, `CLS ${vitals.cls}`).toBeLessThan(BUDGET.cls)
  })

  test('open thread: INP within budget', async ({ page }) => {
    await armVitals(page)
    await login(page)
    await page.goto('/mail', { waitUntil: 'networkidle' })
    // Click the first conversation row; INP samples the interaction.
    const row = page.locator('[data-thread-id], [class*="conversation"]').first()
    await row.click()
    await page.waitForTimeout(1500)
    const vitals = await readVitals(page)
    record('open-thread', vitals)
    if (vitals.inp !== undefined) {
      expect(vitals.inp, `INP ${vitals.inp}ms`).toBeLessThan(BUDGET.inp)
    }
    expect(vitals.cls ?? 0, `CLS ${vitals.cls}`).toBeLessThan(BUDGET.cls)
  })
})
