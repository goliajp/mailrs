// usage: ORIGIN=… TOKEN=<session> bun scripts/cold-load.js
// measures cold-cache first-load timings for each route. each page runs in
// a fresh browser context with cache disabled, so every asset is fetched
// from the network — what a brand-new visitor sees.
import puppeteer from 'puppeteer-core'

const ORIGIN = process.env.ORIGIN || 'https://mail.golia.ai'
const TOKEN = process.env.TOKEN
const ADDRESS = process.env.ADDRESS || 'lihao@golia.jp'
if (!TOKEN) {
  console.error('TOKEN env var is required (POST /api/auth/login → .token)')
  process.exit(2)
}
const AUTH = {
  accessible_domains: (process.env.DOMAINS || 'dadaya.jp,golia.ai,golia.jp').split(','),
  address: ADDRESS,
  display_name: process.env.DISPLAY_NAME || ADDRESS,
  permissions: (process.env.PERMISSIONS || 'mail.read').split(','),
  token: TOKEN,
}

const PAGES = [
  ['/login',                      false],
  ['/dashboard',                  true],
  ['/mail',                       true],
  ['/admin',                      true],
  ['/admin/domains',              true],
  ['/admin/accounts',             true],
  ['/admin/aliases',              true],
  ['/admin/apps',                 true],
  ['/admin/groups',               true],
  ['/admin/email-groups',         true],
  ['/admin/queues',               true],
  ['/admin/audit-log',            true],
  ['/admin/system-config',        true],
  ['/settings',                   true],
]

const browser = await puppeteer.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: 'new',
  args: ['--no-sandbox'],
})

const cols = ['path', 'TTFB', 'FCP', 'LCP', 'DCL', 'Load', 'idle', 'reqs', 'KB', 'CLS', 'CPU']
const w = [22, 6, 6, 6, 6, 6, 6, 5, 7, 6, 6]
const fmt = (row) => row.map((v, i) => String(v).padEnd(w[i])).join(' ')
console.log(fmt(cols))
console.log(fmt(w.map((n) => '-'.repeat(n))))

for (const [path, requiresAuth] of PAGES) {
  const ctx = await browser.createBrowserContext()
  const page = await ctx.newPage()
  await page.setViewport({ width: 1440, height: 900 })
  await page.setCacheEnabled(false)

  // pre-seed localStorage on the origin so SPA boots authenticated.
  // evaluateOnNewDocument runs before any page script, before React mounts.
  if (requiresAuth) {
    await page.evaluateOnNewDocument(
      (auth, host) => {
        if (location.hostname === host) {
          localStorage.setItem('mailrs_auth', JSON.stringify(auth))
        }
      },
      AUTH,
      new URL(ORIGIN).hostname
    )
  }

  // install perf observers before navigation
  await page.evaluateOnNewDocument(() => {
    window.__perf__ = { fcp: 0, lcp: 0, cls: 0 }
    try {
      new PerformanceObserver((list) => {
        for (const e of list.getEntries()) {
          if (e.name === 'first-contentful-paint') window.__perf__.fcp = e.startTime
        }
      }).observe({ type: 'paint', buffered: true })
      new PerformanceObserver((list) => {
        for (const e of list.getEntries()) {
          window.__perf__.lcp = e.startTime
        }
      }).observe({ type: 'largest-contentful-paint', buffered: true })
      new PerformanceObserver((list) => {
        for (const e of list.getEntries()) {
          if (!e.hadRecentInput) window.__perf__.cls += e.value
        }
      }).observe({ type: 'layout-shift', buffered: true })
    } catch (_) {}
  })

  let bytes = 0, reqs = 0
  page.on('response', async (r) => {
    reqs++
    try {
      const buf = await r.buffer()
      bytes += buf.length
    } catch (_) {}
  })

  const t0 = Date.now()
  let timedOut = false
  try {
    await page.goto(`${ORIGIN}${path}`, { waitUntil: 'networkidle2', timeout: 30000 })
  } catch (e) {
    timedOut = true
  }
  const idleAt = Date.now() - t0

  // give LCP one more frame to settle
  await new Promise((r) => setTimeout(r, 400))

  const m = await page.evaluate(() => {
    const nav = performance.getEntriesByType('navigation')[0] || {}
    return {
      ttfb: Math.round(nav.responseStart || 0),
      dcl: Math.round(nav.domContentLoadedEventEnd || 0),
      load: Math.round(nav.loadEventEnd || 0),
      ...(window.__perf__ || {}),
    }
  })
  const cpu = (await page.metrics()).TaskDuration
  console.log(fmt([
    path,
    timedOut ? 'TO' : m.ttfb,
    m.fcp ? Math.round(m.fcp) : '-',
    m.lcp ? Math.round(m.lcp) : '-',
    m.dcl,
    m.load || '-',
    idleAt,
    reqs,
    (bytes / 1024).toFixed(1),
    (m.cls || 0).toFixed(3),
    Math.round(cpu * 1000),
  ]))
  await ctx.close()
}

await browser.close()
