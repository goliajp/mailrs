import type { Browser, Page } from '@playwright/test'

import { expect, test } from '@playwright/test'

/**
 * The mail page against a real local stack (see playwright.stack.config.ts):
 * list tab labels in every language, the filter panel's place beside the
 * folder column, the reply box under a conversation, and a read state
 * reaching a second open page through the event socket.
 *
 * Screenshots go to STACK_SHOTS (default perf-results/stack-shots) for a
 * person to look at; the assertions do not depend on them.
 */

const BASE = process.env.LATENCY_BASE
const USER = process.env.LATENCY_USER ?? 'bench@latency.local'
const PASS = process.env.LATENCY_PASS ?? 'bench-password'
const SHOTS = process.env.STACK_SHOTS ?? 'perf-results/stack-shots'

const WIDTHS = [375, 768, 1280, 1920]
const SCHEMES = ['light', 'dark'] as const
type Scheme = (typeof SCHEMES)[number]

test.skip(!BASE, 'needs LATENCY_BASE: run through scripts/interaction-latency.sh')

async function signedIn(
  browser: Browser,
  width: number,
  scheme: Scheme,
  lang: 'en' | 'ja' | 'zh'
): Promise<Page> {
  const context = await browser.newContext({
    baseURL: BASE,
    colorScheme: scheme,
    viewport: { height: 900, width },
  })
  const page = await context.newPage()
  const res = await page.request.post('/api/auth/login', {
    data: { address: USER, password: PASS },
  })
  expect(res.ok(), `login answered ${res.status()}`).toBe(true)
  const { token: _token, ...auth } = (await res.json()) as Record<string, unknown>
  await page.addInitScript(
    ([a, l]) => {
      localStorage.setItem('mailrs_auth', JSON.stringify(a))
      localStorage.setItem('mailrs_lang', l as string)
    },
    [auth, lang] as const
  )
  await page.goto('/mail')
  await expect(page.locator('[role="listitem"]').first()).toBeVisible({ timeout: 60_000 })
  return page
}

const rows = (page: Page) => page.locator('[role="listitem"] > button[aria-label]')

for (const lang of ['ja', 'zh', 'en'] as const) {
  test(`every list tab label shows whole in ${lang}`, async ({ browser }) => {
    for (const width of WIDTHS) {
      for (const scheme of SCHEMES) {
        const page = await signedIn(browser, width, scheme, lang)
        const tabs = page.locator('[role="tablist"] > [role="tab"]')
        await page.screenshot({ path: `${SHOTS}/tabs-${lang}-${width}-${scheme}.png` })
        // from xl the folder column replaces the tabs
        if (await tabs.first().isVisible()) {
          // The text's own box against the button's content box. A button's
          // scrollWidth does not reliably grow past an ellipsis, so it is
          // not the measure: 通知与… passed it.
          const cut = await tabs.evaluateAll((els) =>
            els
              .filter((el) => {
                const range = document.createRange()
                range.selectNodeContents(el)
                const text = range.getBoundingClientRect()
                const box = el.getBoundingClientRect()
                const style = getComputedStyle(el)
                const inner =
                  box.width - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight)
                // a tab the layout hides (display: none) has no box at all
                if (box.width === 0) return false
                return text.width > inner + 0.5 || text.height > box.height + 0.5
              })
              .map((el) => el.textContent)
          )
          expect(cut, `cut at ${width}px ${scheme}`).toEqual([])
        }
        await page.context().close()
      }
    }
  })
}

test('the filter panel opens inside the list column, beside the folders', async ({ browser }) => {
  for (const width of [1280, 1440, 1920]) {
    for (const scheme of SCHEMES) {
      const page = await signedIn(browser, width, scheme, 'en')
      const toggle = page.getByRole('button', { name: 'Show or hide filters' })
      await toggle.click()
      const panel = toggle.locator('xpath=following-sibling::div[1]')
      await expect(panel).toBeVisible()
      const list = page.locator('[role="listitem"]').first()
      const p = await panel.boundingBox()
      const l = await list.boundingBox()
      expect(p && l, 'measured').toBeTruthy()
      if (p && l) {
        expect(p.x, `${width}px: panel starts left of the list`).toBeGreaterThanOrEqual(l.x - 1)
        expect(p.x + p.width).toBeLessThanOrEqual(width)
      }
      await page.screenshot({ path: `${SHOTS}/filters-${width}-${scheme}.png` })
      await page.context().close()
    }
  }
})

test('the reply box under a conversation keeps its height', async ({ browser }) => {
  for (const width of WIDTHS) {
    for (const scheme of SCHEMES) {
      const page = await signedIn(browser, width, scheme, 'en')
      // near the left edge: the row's own buttons sit over its middle on a phone
      await rows(page)
        .nth(2)
        .click({ position: { x: 24, y: 24 } })
      // a phone shows the conversation with a Reply button instead of a box
      if (width < 768) await page.getByRole('button', { exact: true, name: 'Reply' }).click()
      const editor = page.locator('textarea').last()
      // the box is rendered again once a saved draft is looked for, so it is
      // found afresh on every try
      await expect
        .poll(
          () =>
            editor.evaluate((el) => {
              el.scrollIntoView({ block: 'center' })
              return el.getBoundingClientRect().height
            }),
          { message: `${width}px ${scheme}` }
        )
        .toBeGreaterThanOrEqual(240)
      await page.screenshot({ path: `${SHOTS}/reply-${width}-${scheme}.png` })
      await page.context().close()
    }
  }
})

test('a conversation read on one page shows read on another without a reload', async ({
  browser,
}) => {
  const watcher = await signedIn(browser, 1280, 'light', 'en')
  const reader = await signedIn(browser, 1280, 'light', 'en')
  const unread = watcher.locator('[role="listitem"] > button[aria-label*=" unread"]')
  const label = await unread.first().getAttribute('aria-label')
  expect(label, 'an unread conversation near the top').toBeTruthy()
  const subject = (label ?? '').replace(/, \d+ unread$/, '')

  await reader.locator(`[aria-label="${label}"]`).click()
  // the reader's own list says read first
  await expect(reader.locator(`[aria-label="${subject}"]`)).toHaveCount(1)
  // and the other page, which did nothing, follows
  await expect(watcher.locator(`[aria-label="${subject}"]`)).toHaveCount(1, { timeout: 20_000 })
  await expect(watcher.locator(`[aria-label="${label}"]`)).toHaveCount(0)
  await watcher.screenshot({ path: `${SHOTS}/read-elsewhere-watcher.png` })
  await watcher.context().close()
  await reader.context().close()
})

test('an all-day event saved over CalDAV shows on its days as all day', async ({ browser }) => {
  const setup = await signedIn(browser, 1280, 'light', 'en')
  const ics = [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    'BEGIN:VEVENT',
    'UID:stack-trip',
    'DTSTAMP:20260801T000000Z',
    'DTSTART;VALUE=DATE:20260902',
    'DTEND;VALUE=DATE:20260904',
    'SUMMARY:Trip',
    'END:VEVENT',
    'END:VCALENDAR',
    '',
  ].join('\r\n')
  // straight to webapi: the preview server proxies /api only
  const api = process.env.STACK_API ?? 'http://127.0.0.1:6040'
  const put = await setup.request.put(`${api}/dav/calendars/${USER}/default/stack-trip.ics`, {
    data: ics,
    headers: {
      authorization: `Basic ${Buffer.from(`${USER}:${PASS}`).toString('base64')}`,
      'content-type': 'text/calendar',
    },
  })
  expect(put.status(), await put.text()).toBe(201)
  const listed = await setup.request.get(
    '/api/calendar/events?start=2026-08-31T00:00:00Z&end=2026-09-07T00:00:00Z'
  )
  const events = (await listed.json()) as { dtend: string; dtstart: string; uid: string }[]
  expect(events.map((e) => [e.uid, e.dtstart, e.dtend])).toEqual([
    ['stack-trip', '2026-09-02', '2026-09-04'],
  ])
  await setup.context().close()

  for (const width of WIDTHS) {
    for (const scheme of SCHEMES) {
      const page = await signedIn(browser, width, scheme, 'en')
      await page.goto('/calendar?date=2026-09-02')
      // only the chips on screen: the document holds as many again, not displayed
      const chips = page
        .getByTestId('calendar-event')
        .filter({ hasText: 'Trip' })
        .filter({ visible: true })
      await expect(chips).toHaveCount(2)
      await expect(chips.first()).toContainText('All day')
      await chips.first().click()
      await expect(page.getByRole('region', { name: 'Trip' })).toContainText('All day')
      await page.screenshot({ path: `${SHOTS}/calendar-${width}-${scheme}.png` })
      await page.context().close()
    }
  }
})
