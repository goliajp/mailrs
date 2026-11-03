import type { Page } from '@playwright/test'

import { expect, test } from '@playwright/test'

/**
 * The reading pane's message header in a narrow window.
 *
 * At 768 px the reading pane is about 210 px wide. The five header
 * actions kept their width and squeezed the sender and date column to
 * nothing, so the date was laid out one character per line — 0 px wide
 * and 144 px tall in Chinese. Measured in a browser, because jsdom does
 * no layout and would report every box as 0 x 0 whether broken or not.
 */

const MESSAGES = [
  {
    attachments: [],
    html_body: '',
    internal_date: 1754400000,
    recipients: 'a@golia.jp',
    sender: '山田 太郎 <taro@example.co.jp>',
    subject: '請求書の件',
    text_body: 'ご確認ください。',
    uid: 1,
  },
]

const THREADS = [
  {
    last_date: 1754400000,
    message_count: 1,
    participants: [MESSAGES[0].sender],
    received_count: 1,
    snippet: 'ご確認ください。',
    subject: MESSAGES[0].subject,
    thread_id: 't1',
    unread_count: 1,
  },
]

async function openConversation(page: Page, lang: string) {
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
  await page.addInitScript((l) => {
    localStorage.setItem('mailrs_lang', l)
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
  }, lang)
  await page.goto('/mail')
  const row = page.locator('[role="listitem"]').first()
  await row.waitFor()
  const box = await row.boundingBox()
  if (!box) throw new Error('no conversation row to open')
  await page.mouse.click(box.x + 60, box.y + box.height / 2)
}

test.use({ viewport: { height: 1024, width: 768 } })

for (const lang of ['zh', 'ja', 'en']) {
  test(`the date in the message header reads across, ${lang}`, async ({ page }) => {
    await openConversation(page, lang)
    const date = page.locator('article [data-message-date]').first()
    await expect(date).toBeVisible()
    const box = await date.boundingBox()
    if (!box) throw new Error('the date has no box')
    const lineHeight = await date.evaluate((el) => parseFloat(getComputedStyle(el).lineHeight))
    // a column wide enough for the date's words, and at most one break
    // between date and time
    expect(box.width).toBeGreaterThan(100)
    expect(box.height).toBeLessThanOrEqual(lineHeight * 2 + 1)
  })
}
