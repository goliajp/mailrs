/**
 * Interaction latency of the mail client: P50 / P95 over 30 repetitions of
 * each interaction, from the input event to the first frame painted with
 * the target content (see `latency/probe.ts` for exactly where the clock
 * starts and stops).
 *
 * Conditions: the production build (`vite preview`) in front of a real
 * mailrs-webapi and mailrs-fastcore holding ~30,000 synthetic conversations,
 * CPU throttled 4x and the network shaped to Fast 4G. Nothing is mocked.
 * `scripts/interaction-latency.sh` boots all of that and runs this spec;
 * without LATENCY_BASE it skips.
 *
 * Machine load: load1 is sampled every second. If it is over
 * LATENCY_MAX_LOAD at the start, nothing is measured; if it went over
 * during the run, the numbers are written but the verdict is "not
 * measured", which the wrapper turns into exit status 3. Neither is a pass.
 *
 * The budgets are the acceptance values. This spec reports against them
 * and does not fail on them yet.
 */

import type { ProbeResult } from './latency/probe'
import type { Sample, Summary } from './latency/report'
import type { CDPSession, Page } from '@playwright/test'

import { expect, test } from '@playwright/test'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { installProbe } from './latency/probe'
import { LoadWitness, summarize } from './latency/report'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

const BASE = process.env.LATENCY_BASE
const USER = process.env.LATENCY_USER ?? 'bench@latency.local'
const PASS = process.env.LATENCY_PASS ?? 'bench-password'
const RUNS = Number(process.env.LATENCY_RUNS ?? 30)

/** Acceptance values, ms (U4.1). */
const BUDGET = { interaction: 100, search: 300 }

/**
 * Highest load1 a run may see and still count.
 *
 * The run itself is the browser, node, fastcore, webapi, kevy and vite
 * preview; on an idle 14-core Mac mini it raises load1 to about 2. CPU
 * throttling is applied by pausing the renderer, so it does not add
 * load. A quarter of the logical cores leaves the renderer's main thread
 * a performance core of its own with room to spare; past that, a slower
 * interaction cannot be told apart from a busier machine.
 */
const MAX_LOAD = Number(process.env.LATENCY_MAX_LOAD ?? os.cpus().length / 4)

/** CPU 4x; Fast 4G: 9 Mbps down, 1.5 Mbps up, 170 ms RTT. */
const CPU_RATE = 4
const NETWORK = {
  downloadThroughput: (9_000_000 / 8) | 0,
  latency: 170,
  offline: false,
  uploadThroughput: (1_500_000 / 8) | 0,
}

// non-empty suffix only: a bare prefix like "alpha" would also match "alphax"
const PREFIXES = [
  'alpha',
  'bravo',
  'cedar',
  'delta',
  'ember',
  'fjord',
  'garnet',
  'harbor',
  'indigo',
  'juniper',
  'kestrel',
  'lumen',
  'marble',
  'nimbus',
  'onyx',
  'prism',
  'quartz',
  'raven',
  'sierra',
  'tundra',
]
const SUFFIXES = ['x', 'line', 'port', 'field', 'stone', 'wave', 'gate', 'mark', 'core']
const TERMS = Array.from(
  { length: RUNS },
  (_, i) => `${PREFIXES[i % PREFIXES.length]}${SUFFIXES[i % SUFFIXES.length]}`
)

type Name = 'archive' | 'markRead' | 'nextPage' | 'open' | 'search' | 'switch'
const results: Partial<Record<Name, { samples: Sample[]; summary: Summary }>> = {}
const witness = new LoadWitness()
let startLoad = 0
let browserVersion = ''

type Lat = {
  arm: (e: string, p: string, a: string, t: number) => void
  now: () => number
  predicates: Record<string, (arg: string) => boolean>
  resourcesBetween: (a: number, b: number) => ProbeResult['resources']
  result: () => Promise<ProbeResult>
}
declare global {
  var __lat: Lat
}

test.describe.configure({ mode: 'serial' })
test.use({ locale: 'en-US', timezoneId: 'Asia/Tokyo', viewport: { height: 900, width: 1440 } })
test.skip(!BASE, 'LATENCY_BASE unset: run scripts/interaction-latency.sh')

test.beforeAll(async ({ browser }) => {
  test.setTimeout(5 * 60_000)
  browserVersion = browser.version()
  // Seeding and booting the stack just before this raise load1, and it
  // decays with a one-minute time constant: give it up to three minutes.
  const until = Date.now() + 3 * 60_000
  while (witness.now() > MAX_LOAD && Date.now() < until) {
    await new Promise((r) => setTimeout(r, 5000))
  }
  witness.start()
  startLoad = witness.now()
})

test.afterAll(() => {
  if (!BASE) return
  const load = witness.stop()
  const measured = load.start <= MAX_LOAD && load.max <= MAX_LOAD
  const verdict = measured ? 'measured' : 'not measured'
  const out = {
    at: new Date().toISOString(),
    browser: `chromium ${browserVersion}`,
    budget: BUDGET,
    conditions: { cpuThrottle: CPU_RATE, network: NETWORK, runs: RUNS },
    load: { ...load, limit: MAX_LOAD },
    machine: {
      arch: os.arch(),
      cpu: os.cpus()[0]?.model,
      cpus: os.cpus().length,
      host: os.hostname(),
      memGiB: Math.round(os.totalmem() / 2 ** 30),
      platform: `${os.platform()} ${os.release()}`,
    },
    results,
    verdict,
  }
  const dir = path.join(__dirname, '../perf-results')
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, 'interaction-latency.json'), JSON.stringify(out, null, 2))
  printTable(load)
  if (!measured) {
    console.log(
      `\n!! 未测 / not measured: load1 start ${load.start.toFixed(2)}, max ${load.max.toFixed(2)}` +
        ` > limit ${MAX_LOAD}. These numbers are not a baseline.`
    )
  }
  // a repetition whose target never painted has no latency; say so
  const broken = Object.entries(results).filter(([, r]) => r.summary.timeouts > 0)
  if (broken.length > 0) {
    throw new Error(
      broken.map(([n, r]) => `${n}: ${r.summary.timeouts} of ${RUNS} never painted`).join('; ')
    )
  }
})

type Rig = { cdp: CDPSession; inflight: Set<string>; lastNetwork: number; page: Page }

// ── harness ─────────────────────────────────────────────────────────

async function boot(page: Page): Promise<Rig> {
  if (startLoad > MAX_LOAD) {
    test.skip(true, `未测 / not measured: load1 ${startLoad.toFixed(2)} > ${MAX_LOAD} at start`)
  }
  const res = await page.request.post('/api/auth/login', {
    data: { address: USER, password: PASS },
  })
  expect(res.ok(), `login answered ${res.status()}`).toBe(true)
  const { token: _token, ...auth } = (await res.json()) as Record<string, unknown>
  await page.addInitScript(installProbe)
  await page.addInitScript((a) => localStorage.setItem('mailrs_auth', JSON.stringify(a)), auth)

  const cdp = await page.context().newCDPSession(page)
  const rig: Rig = { cdp, inflight: new Set(), lastNetwork: Date.now(), page }
  cdp.on('Network.requestWillBeSent', (e) => {
    rig.inflight.add(e.requestId)
    rig.lastNetwork = Date.now()
  })
  const done = (e: { requestId: string }) => {
    rig.inflight.delete(e.requestId)
    rig.lastNetwork = Date.now()
  }
  cdp.on('Network.loadingFinished', done)
  cdp.on('Network.loadingFailed', done)
  await cdp.send('Network.enable')
  await cdp.send('Performance.enable')
  await cdp.send('Network.emulateNetworkConditions', NETWORK)
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: CPU_RATE })

  await page.goto('/mail')
  await expect(page.locator('[role="listitem"]').first()).toBeVisible({ timeout: 60_000 })
  await expect(
    page.locator('[role="group"][aria-label="Messages in this conversation"]').first()
  ).toBeVisible({ timeout: 60_000 })
  await settle(rig)
  return rig
}

async function mainThread(cdp: CDPSession) {
  const { metrics } = await cdp.send('Performance.getMetrics')
  const get = (n: string) => (metrics.find((m) => m.name === n)?.value ?? 0) * 1000
  return {
    layout: get('LayoutDuration'),
    script: get('ScriptDuration'),
    style: get('RecalcStyleDuration'),
    task: get('TaskDuration'),
  }
}

function printTable(load: { end: number; max: number; start: number }) {
  console.log(
    `\nload1 start ${load.start.toFixed(2)} max ${load.max.toFixed(2)} end ${load.end.toFixed(2)}` +
      ` (limit ${MAX_LOAD}), ${os.cpus().length} cpus`
  )
  console.log(
    'interaction  n   P50    P95   budget  req(crit/all)  KB(crit/all)  main-thread ms (script/style/layout/other)'
  )
  for (const [name, r] of Object.entries(results)) {
    const s = r.summary
    const budget = name === 'search' ? BUDGET.search : BUDGET.interaction
    const m = s.mainThread
    console.log(
      `${name.padEnd(11)} ${String(s.n).padStart(2)} ${s.p50.toFixed(0).padStart(5)} ${s.p95
        .toFixed(0)
        .padStart(
          6
        )}  ${String(budget).padStart(5)}  ${`${s.requests.critical}/${s.requests.triggered}`.padStart(
        12
      )}  ${`${(s.bytes.critical / 1024).toFixed(1)}/${(s.bytes.triggered / 1024).toFixed(1)}`.padStart(
        12
      )}  ${m.total.toFixed(0)} (${m.script.toFixed(0)}/${m.style.toFixed(0)}/${m.layout.toFixed(0)}/${m.other.toFixed(0)})` +
        (s.timeouts ? `  ${s.timeouts} timed out` : '')
    )
  }
}

/** Quiet network for 500 ms, then two frames. */
async function settle(rig: Rig) {
  const until = Date.now() + 20_000
  while (Date.now() < until) {
    if (rig.inflight.size === 0 && Date.now() - rig.lastNetwork > 500) break
    await rig.page.waitForTimeout(100)
  }
  await rig.page.evaluate(
    () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
  )
}

const bytes = (rs: ProbeResult['resources']) => rs.reduce((a, r) => a + r.transferSize, 0)

type RowInfo = { current: boolean; index: number; n: number; unread: boolean; visible: boolean }

/** One repetition: arm, act, wait for the paint, then account for it. */
async function measure(
  rig: Rig,
  eventType: string,
  predicate: string,
  arg: string,
  act: () => Promise<void>
): Promise<null | Sample> {
  const { cdp, page } = rig
  const before = await mainThread(cdp)
  await page.evaluate(([e, p, a]) => window.__lat.arm(e, p, a, 20_000), [eventType, predicate, arg])
  await act()
  const r = await page.evaluate(() => window.__lat.result())
  const after = await mainThread(cdp)
  await settle(rig)
  if (r.timedOut) {
    const seen = await page.evaluate(() => ({
      current: document.querySelector('[aria-current="true"]')?.getAttribute('aria-label'),
      focus: document.activeElement?.outerHTML.slice(0, 120),
      pane: /REF-\d+/.exec(
        document.querySelector('[role="group"][aria-label="Messages in this conversation"]')
          ?.textContent ?? ''
      )?.[0],
      paneText: document
        .querySelector('[role="group"][aria-label="Messages in this conversation"]')
        ?.textContent?.slice(0, 300),
    }))
    console.log(`timed out waiting for ${predicate}(${arg}):`, JSON.stringify(seen))
    return null
  }
  const now = await page.evaluate(() => window.__lat.now())
  const critical = await page.evaluate(
    ([a, b]) => window.__lat.resourcesBetween(a, b),
    [r.start, r.end]
  )
  const all = await page.evaluate(([a, b]) => window.__lat.resourcesBetween(a, b), [r.start, now])
  const ms = r.end - r.start

  // phases: only responses that arrived before the paint were waited for
  const awaited = critical.filter((x) => x.name.includes('/api/') && x.responseEnd <= r.end)
  let phases = { local: ms, network: 0, render: 0, toRequest: 0 }
  if (awaited.length > 0) {
    const first = Math.min(...awaited.map((x) => x.startTime))
    const last = Math.max(...awaited.map((x) => x.responseEnd))
    phases = { local: 0, network: last - first, render: r.end - last, toRequest: first - r.start }
  }
  const d = {
    layout: after.layout - before.layout,
    script: after.script - before.script,
    style: after.style - before.style,
    task: after.task - before.task,
  }
  return {
    critical: { bytes: bytes(critical), requests: critical.length },
    event: r.event
      ? {
          delay: r.event.processingStart - r.start,
          handlers: r.event.processingEnd - r.event.processingStart,
          presentation: r.start + r.event.duration - r.event.processingEnd,
        }
      : null,
    mainThread: {
      layout: d.layout,
      other: Math.max(0, d.task - d.script - d.style - d.layout),
      script: d.script,
      style: d.style,
      total: d.task,
    },
    ms,
    phases,
    triggered: {
      bytes: bytes(all),
      requests: all.length,
      urls: all.map((x) => new URL(x.name).pathname),
    },
  }
}

async function repeat(name: Name, one: (i: number) => Promise<null | Sample>) {
  const samples: Sample[] = []
  let timeouts = 0
  for (let i = 0; i < RUNS; i++) {
    const s = await one(i)
    if (s) samples.push(s)
    else timeouts += 1
  }
  results[name] = { samples, summary: summarize(samples, timeouts) }
}

/** The rendered rows, in list order. */
function rows(page: Page): Promise<RowInfo[]> {
  return page.evaluate(() => {
    const list = document.querySelector('[role="list"][aria-label]')
    const box = list?.getBoundingClientRect()
    const out: RowInfo[] = []
    for (const slot of Array.from(document.querySelectorAll<HTMLElement>('[data-index]'))) {
      const b = slot.querySelector<HTMLButtonElement>('[role="listitem"] > button')
      if (!b) continue
      const label = b.getAttribute('aria-label') ?? ''
      const m = /#(\d+)(?:,|$)/.exec(label)
      if (!m) continue
      const r = slot.getBoundingClientRect()
      out.push({
        current: b.getAttribute('aria-current') === 'true',
        index: Number(slot.dataset.index),
        n: Number(m[1]),
        unread: /, \d+ unread/.test(label),
        visible: !!box && r.top >= box.top && r.bottom <= box.bottom,
      })
    }
    return out.sort((a, b) => a.index - b.index)
  })
}

const rowButton = (page: Page, n: number) =>
  page.getByRole('button', { exact: false, name: new RegExp(`#${n}(,|$)`) })

async function scrollList(page: Page, to: 'bottom' | number) {
  await page.evaluate((t) => {
    const list = document.querySelector<HTMLElement>('[role="list"][aria-label]')
    if (!list) return
    list.scrollTop = t === 'bottom' ? list.scrollHeight : list.scrollHeight - list.clientHeight - t
  }, to)
}

// ── interactions ────────────────────────────────────────────────────
// Mark read runs first: every other interaction opens conversations, and
// opening one marks it read on the server, which this run shares.

test('mark read (context menu, row loses its unread state)', async ({ page }) => {
  test.setTimeout(15 * 60_000)
  const rig = await boot(page)
  await repeat('markRead', async () => {
    let row = (await rows(page)).find((r) => r.unread && !r.current)
    for (let tries = 0; !row && tries < 5; tries++) {
      await scrollList(page, 'bottom')
      await settle(rig)
      row = (await rows(page)).find((r) => r.unread && !r.current)
    }
    if (!row) throw new Error('no unread row left to mark read')
    const target = rowButton(page, row.n)
    await target.scrollIntoViewIfNeeded()
    await target.click({ button: 'right' })
    const item = page.getByRole('menuitem', { name: 'Mark as read' })
    await expect(item).toBeVisible()
    await settle(rig)
    return measure(rig, 'click', 'rowRead', String(row.n), () => item.click())
  })
})

test('switch conversation (j, reading pane shows the next one)', async ({ page }) => {
  test.setTimeout(15 * 60_000)
  const rig = await boot(page)
  await repeat('switch', async () => {
    const list = await rows(page)
    const at = list.findIndex((r) => r.current)
    const next = list[at + 1]
    if (!next) throw new Error('no next row')
    return measure(rig, 'keydown', 'paneShows', String(next.n), () => page.keyboard.press('j'))
  })
})

test('open conversation (click a row three below)', async ({ page }) => {
  test.setTimeout(15 * 60_000)
  const rig = await boot(page)
  await repeat('open', async () => {
    // the current row to the top, so the rows below it are rendered
    await page.evaluate(() =>
      document.querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'start' })
    )
    await settle(rig)
    const list = await rows(page)
    const at = list.findIndex((r) => r.current)
    const pick = list[at + 3]
    if (!pick) throw new Error('no row three below the current one')
    const target = rowButton(page, pick.n)
    await target.scrollIntoViewIfNeeded()
    await settle(rig)
    return measure(rig, 'click', 'paneShows', String(pick.n), () => target.click())
  })
})

test('archive (e, row leaves the list)', async ({ page }) => {
  test.setTimeout(15 * 60_000)
  const rig = await boot(page)
  await repeat('archive', async () => {
    const cur = (await rows(page)).find((r) => r.current)
    if (!cur) throw new Error('nothing selected')
    await page.waitForFunction((n) => window.__lat.predicates.paneShows(n), String(cur.n))
    await settle(rig)
    return measure(rig, 'keydown', 'rowGone', String(cur.n), () => page.keyboard.press('e'))
  })
})

test('next page (wheel to the bottom, new rows painted)', async ({ page }) => {
  test.setTimeout(15 * 60_000)
  const rig = await boot(page)
  const list = page.locator('[role="list"][aria-label]')
  await repeat('nextPage', async () => {
    // sentinel 600 px below the fold: outside the observer's 300 px
    // margin, and close enough that overscan renders the last loaded row
    await scrollList(page, 600)
    await settle(rig)
    const oldest = Math.min(...(await rows(page)).map((r) => r.n))
    const box = await list.boundingBox()
    if (!box) throw new Error('no list')
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    return measure(rig, 'wheel', 'listHasNewPage', String(oldest), () => page.mouse.wheel(0, 2000))
  })
})

test('search first screen (query entered, first results painted)', async ({ page }) => {
  test.setTimeout(15 * 60_000)
  const rig = await boot(page)
  const input = page.getByRole('search').locator('input')
  await repeat('search', async (i) => {
    await input.click()
    await settle(rig)
    const s = await measure(rig, 'input', 'searchShows', TERMS[i], () => input.fill(TERMS[i]))
    await input.fill('')
    await settle(rig)
    return s
  })
})
