// usage: ORIGIN=… TOKEN=<session> ADDRESS=user@domain bun scripts/page-perf.js
// TOKEN is required — get one from POST /api/auth/login. The AUTH object
// mirrors the SPA's localStorage('mailrs_auth') shape; the server only
// trusts the token, the other fields just keep the client code happy.
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

// seed localStorage on origin
const seed = await browser.newPage()
await seed.goto(`${ORIGIN}/login`, { waitUntil: 'domcontentloaded' })
await seed.evaluate((auth) => localStorage.setItem('mailrs_auth', JSON.stringify(auth)), AUTH)
await seed.close()

const head = ['path', 'TTFB', 'DCL', 'Load', 'FCP', 'LCP', 'reqs', 'transfer_kb', 'cpu_ms']
const w = [28, 6, 6, 6, 6, 6, 5, 12, 7]
const fmt = (row) => row.map((v, i) => String(v).padEnd(w[i])).join(' ')
console.log(fmt(head))
console.log(fmt(w.map((n) => '-'.repeat(n))))

for (const [path, _auth] of PAGES) {
  const page = await browser.newPage()
  await page.setViewport({ width: 1440, height: 900 })

  let bytes = 0, reqs = 0
  page.on('response', async (r) => {
    reqs++
    const cl = r.headers()['content-length']
    if (cl) bytes += parseInt(cl, 10)
  })

  const t0 = Date.now()
  try {
    await page.goto(`${ORIGIN}${path}`, { waitUntil: 'networkidle2', timeout: 30000 })
  } catch (e) {
    console.log(fmt([path, 'TIMEOUT', '-', '-', '-', '-', reqs, '-', '-']))
    await page.close()
    continue
  }
  // give app a beat to settle paints/long tasks
  await new Promise((r) => setTimeout(r, 300))

  const m = await page.evaluate(() => {
    const nav = performance.getEntriesByType('navigation')[0] || {}
    const paints = performance.getEntriesByType('paint')
    const fcp = paints.find((p) => p.name === 'first-contentful-paint')?.startTime
    const lcp = (() => {
      // fallback if not observed
      try {
        const obs = performance.getEntriesByType('largest-contentful-paint')
        return obs[obs.length - 1]?.startTime
      } catch { return undefined }
    })()
    return {
      ttfb: Math.round(nav.responseStart || 0),
      dcl: Math.round(nav.domContentLoadedEventEnd || 0),
      load: Math.round(nav.loadEventEnd || 0),
      fcp: fcp ? Math.round(fcp) : '-',
      lcp: lcp ? Math.round(lcp) : '-',
    }
  })
  const cpu = (await page.metrics()).TaskDuration
  const wallMs = Date.now() - t0
  console.log(fmt([
    path,
    m.ttfb, m.dcl, m.load,
    m.fcp, m.lcp,
    reqs,
    (bytes / 1024).toFixed(1),
    Math.round(cpu * 1000),
  ]))
  await page.close()
}

await browser.close()
