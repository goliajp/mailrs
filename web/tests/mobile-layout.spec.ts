import type { Page } from '@playwright/test'

import { expect, test } from '@playwright/test'

/**
 * Nothing on a phone may be wider than the phone.
 *
 * Driven through the real app — real router, real components, real
 * wire schemas — with the API stubbed and an auth blob seeded into
 * localStorage. The data is deliberately hostile: 300-character
 * subjects, a 120-character sender address, an unbroken 180-character
 * URL, CJK runs, an email authored at 760 px, and attachment filenames
 * long enough to be their own paragraph.
 *
 * Every check asserts the content is actually on screen before it
 * measures. Without that this suite passes at its brightest when the
 * stubs stop matching and every screen renders empty — the failure mode
 * `measure-before-you-cut-over` calls a verification that cannot come
 * out zero.
 */

const PHONES = [
  { height: 812, name: 'iPhone 13 mini', width: 375 },
  { height: 932, name: 'iPhone 15 Pro Max', width: 430 },
]

const WIDE_EMAIL = `<table width="760" style="width:760px"><tr><td>
  <div style="width:760px;background:#eef;padding:8px"><h1>Newsletter</h1>
  <p>${'lorem ipsum dolor sit amet '.repeat(15)}</p></div></td></tr></table>`

const MESSAGES = [
  {
    attachments: [
      { filename: `${'Very_Long_Attachment_Filename_'.repeat(5)}final_v3.pdf`, index: 0, size: 123456 },
      { filename: '請求書_2026年8月分_株式会社ゴリア様_確定版.xlsx', index: 1, size: 4096 },
    ],
    html_body: WIDE_EMAIL,
    internal_date: 1754400000,
    recipients: Array.from({ length: 8 }, (_, i) => `recipient${i}@example.com`).join(', '),
    sender: 'verylongsender.with.dots@a-really-long-subdomain.example-company.co.jp',
    subject: `${'Q3 '.repeat(25)}report`,
    text_body: '',
    uid: 1,
  },
]

const THREADS = [
  {
    last_date: 1754400000,
    message_count: 1,
    participants: [MESSAGES[0].sender],
    received_count: 1,
    snippet: `https://example.com/${'a'.repeat(180)}`,
    subject: MESSAGES[0].subject,
    thread_id: 't1',
    unread_count: 1,
  },
  {
    last_date: 1754300000,
    message_count: 1,
    participants: ['a@b.jp', 'c@d.jp', 'e@f.jp'],
    received_count: 1,
    snippet: 'ご確認ください。'.repeat(20),
    subject: 'x'.repeat(300),
    thread_id: 't2',
    unread_count: 0,
  },
]

async function stubApi(page: Page) {
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    let json: unknown = {}
    if (/categories/.test(path)) json = []
    else if (/conversations\/t1$/.test(path)) json = MESSAGES
    else if (/conversations$/.test(path)) {
      json = { has_more: false, items: THREADS, next_cursor: null }
    }
    await route.fulfill({ json, status: 200 })
  })
  await page.addInitScript(() => {
    localStorage.setItem(
      'mailrs_auth',
      JSON.stringify({
        accessible_domains: ['golia.jp'],
        address: 'a@golia.jp',
        display_name: 'A',
        permissions: [],
        token: 'stub',
      })
    )
  })
}

/**
 * Everything painting outside the viewport, ignoring anything inside a
 * horizontal scroller — a wide table in an `overflow-x: auto` box is
 * reachable, which is the whole point of putting it in one.
 */
async function offenders(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const vw = document.documentElement.clientWidth
    const out: string[] = []
    for (const el of document.querySelectorAll('*')) {
      const r = el.getBoundingClientRect()
      if (r.width === 0 || r.height === 0) continue
      let a = el.parentElement
      let scrollable = false
      while (a) {
        const o = getComputedStyle(a).overflowX
        if (o === 'auto' || o === 'scroll') {
          scrollable = true
          break
        }
        a = a.parentElement
      }
      if (!scrollable && (r.right > vw + 1 || r.left < -1)) {
        const cls = String((el as HTMLElement).className).slice(0, 48)
        out.push(`${el.tagName.toLowerCase()}[${cls}] ${Math.round(r.left)}..${Math.round(r.right)}`)
      }
    }
    return out
  })
}

for (const phone of PHONES) {
  test.describe(phone.name, () => {
    test.use({ viewport: { height: phone.height, width: phone.width } })

    test('the conversation list survives hostile subjects and addresses', async ({ page }) => {
      await stubApi(page)
      await page.goto('/mail')
      await expect(page.locator('[role="listitem"]')).toHaveCount(THREADS.length)
      expect(await offenders(page)).toEqual([])
    })

    test('a 760px email is scaled to the column, attachments and all', async ({ page }) => {
      await stubApi(page)
      await page.goto('/mail')
      await page.locator('[role="listitem"] button').first().click()

      // Liveness first: the body has to be on screen before its width
      // means anything. A hidden host measures 0 and would "fit".
      const body = await page.waitForFunction(() => {
        const host = [...document.querySelectorAll('div')].find(
          (d) => d.shadowRoot?.querySelector('.mail-wrap') && d.clientWidth > 0
        )
        if (!host) return null
        const wrap = host.shadowRoot?.querySelector('.mail-wrap') as HTMLElement
        const m = /scale\(([\d.]+)\)/.exec(wrap.style.transform)
        return {
          boxWidth: Math.round(wrap.getBoundingClientRect().width),
          columnWidth: host.clientWidth,
          scale: m ? Number(m[1]) : 1,
        }
      })
      const { boxWidth, columnWidth, scale } = await body.jsonValue()

      expect(scale).toBeLessThan(1)
      expect(boxWidth).toBe(columnWidth)
      // Filtered to visible, not just `.first()`: the app keeps the
      // mobile and desktop panes both mounted, so the first match is a
      // hidden copy and asserting on it proves nothing.
      for (const name of ['.pdf', '.xlsx']) {
        const shown = page.getByText(name, { exact: false }).filter({ visible: true })
        await expect(shown.first()).toBeVisible()
      }
      expect(await offenders(page)).toEqual([])
    })

    for (const path of ['/settings', '/admin']) {
      test(`${path} fits the screen`, async ({ page }) => {
        await stubApi(page)
        await page.goto(path)
        await expect(page.locator('main')).toBeVisible()
        expect(await offenders(page)).toEqual([])
      })
    }
  })
}
