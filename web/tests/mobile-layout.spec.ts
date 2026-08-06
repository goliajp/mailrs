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

/**
 * Tap the row where a thumb would, not at its geometric centre.
 *
 * The row's activation is a button stretched under the content, and the
 * archive / star cluster sits on top of it — near the middle of the row
 * once the date is long. Clicking the element's centre lands on Archive.
 * That is not new: the cluster occupied the same pixels when it was
 * nested inside the row button, and clicking it selected nothing then
 * either.
 */
async function openFirstRow(page: Page) {
  const box = await page.locator('[role="listitem"]').first().boundingBox()
  if (!box) throw new Error('no conversation row to open')
  await page.mouse.click(box.x + 60, box.y + box.height / 2)
}

/** Non-GET requests the app made, in order. */
function recordWrites(page: Page): string[] {
  const writes: string[] = []
  page.on('request', (r) => {
    if (r.method() !== 'GET' && r.url().includes('/api/')) writes.push(new URL(r.url()).pathname)
  })
  return writes
}

/**
 * A horizontal drag across a row, paced like a real one.
 *
 * The events have to be spaced: the commit threshold is read from React
 * state, so dispatching move and end in the same task leaves `handleTouchEnd`
 * looking at the offset from before the drag. That is a property of the
 * test harness, not of a finger.
 */
async function swipeRow(page: Page, index: number, dx: number) {
  const box = await page.locator('[role="listitem"]').nth(index).boundingBox()
  if (!box) throw new Error('no row to swipe')
  const x0 = box.x + 180
  const y0 = box.y + box.height / 2
  const fire = (type: string, x: number) =>
    page.evaluate(
      ([type, x, y, index]) => {
        const row = document.querySelectorAll('[role="listitem"]')[index as number]
        const target = row.parentElement as HTMLElement
        const t = new Touch({ clientX: x as number, clientY: y as number, identifier: 1, target })
        target.dispatchEvent(
          new TouchEvent(type as string, {
            bubbles: true,
            cancelable: true,
            changedTouches: [t],
            targetTouches: type === 'touchend' ? [] : [t],
            touches: type === 'touchend' ? [] : [t],
          })
        )
      },
      [type, x, y0, index] as const
    )
  await fire('touchstart', x0)
  for (let i = 1; i <= 6; i++) {
    await fire('touchmove', x0 + (dx * i) / 6)
    await page.waitForTimeout(30)
  }
  await fire('touchend', x0 + dx)
}

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

    /**
     * A control inside a control is invalid HTML and puts a focusable
     * element where assistive tech expects a label. Every conversation
     * row was a `<button>` wrapping four more until 2026-08-05 — found
     * only because driving the real app surfaced React saying so, not by
     * reading the file.
     */
    test('no control contains another control', async ({ page }) => {
      await stubApi(page)
      await page.goto('/mail')
      await expect(page.locator('[role="listitem"]')).toHaveCount(THREADS.length)
      const nested = await page.evaluate(() =>
        [...document.querySelectorAll('button button, button a[href], a[href] button, a[href] a[href]')].map(
          (el) => `${el.tagName.toLowerCase()} in ${el.closest('button, a[href]')?.tagName.toLowerCase()}`
        )
      )
      expect(nested).toEqual([])
    })

    /**
     * A thread is read when you read it.
     *
     * The phone auto-selects the first row as soon as the list arrives —
     * that is how the reading pane knows what to show once you tap in.
     * It is not permission to mark anything read. Until 2026-08-05 the
     * hidden desktop tree was mounted here, auto-opened that selection
     * and marked it read: arriving at your mailbox on a phone marked the
     * newest thread read, twice, without it ever being on screen.
     */
    test('arriving at the mailbox marks nothing read', async ({ page }) => {
      await stubApi(page)
      const writes = recordWrites(page)
      await page.goto('/mail')
      await expect(page.locator('[role="listitem"]')).toHaveCount(THREADS.length)
      await page.waitForTimeout(800)
      expect(writes.filter((w) => w.endsWith('/read'))).toEqual([])
    })

    test('opening a thread marks it read, once', async ({ page }) => {
      await stubApi(page)
      const writes = recordWrites(page)
      await page.goto('/mail')
      await openFirstRow(page)
      await expect
        .poll(() => writes.filter((w) => w.endsWith('/read')).length, { timeout: 5000 })
        .toBe(1)
      // And stays at one: both shells used to run the effect.
      await page.waitForTimeout(600)
      expect(writes.filter((w) => w.endsWith('/read'))).toEqual(['/api/conversations/t1/read'])
    })

    /**
     * Deleting a thread unlinks its maildir files. There is no trash and
     * nothing to restore from, and the reading pane has always said so
     * before doing it — but the list reached the same verb without
     * asking, so until 2026-08-05 one left swipe on a phone destroyed
     * mail outright.
     */
    test('a left swipe asks before destroying anything', async ({ page }) => {
      await stubApi(page)
      const writes = recordWrites(page)
      await page.goto('/mail')
      await expect(page.locator('[role="listitem"]')).toHaveCount(THREADS.length)

      await swipeRow(page, 1, -110)
      await expect(page.getByText('Delete conversation?')).toBeVisible()
      expect(writes.filter((w) => w.includes('conversations/t'))).toEqual([])

      await page.getByRole('button', { name: 'Cancel' }).click()
      await page.waitForTimeout(300)
      expect(writes.filter((w) => w.includes('conversations/t'))).toEqual([])
    })

    /** Archive is reversible, so it needs no question. */
    test('a right swipe archives straight away', async ({ page }) => {
      await stubApi(page)
      const writes = recordWrites(page)
      await page.goto('/mail')
      await expect(page.locator('[role="listitem"]')).toHaveCount(THREADS.length)

      await swipeRow(page, 1, 110)
      await expect.poll(() => writes.some((w) => w.endsWith('/archive')), { timeout: 4000 }).toBe(true)
    })

    test('the conversation list survives hostile subjects and addresses', async ({ page }) => {
      await stubApi(page)
      await page.goto('/mail')
      await expect(page.locator('[role="listitem"]')).toHaveCount(THREADS.length)
      expect(await offenders(page)).toEqual([])
    })

    test('a 760px email is scaled to the column, attachments and all', async ({ page }) => {
      await stubApi(page)
      await page.goto('/mail')
      await openFirstRow(page)

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
