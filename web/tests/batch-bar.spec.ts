import type { Page } from '@playwright/test'

import { expect, test } from '@playwright/test'

/**
 * The toolbar for picked conversations is one row at every list width,
 * in each language. Measured in a browser, because jsdom does no layout:
 * whether the actions fit is exactly what it cannot say.
 */

const THREADS = Array.from({ length: 6 }, (_, i) => ({
  last_date: 1754400000 - i * 3600,
  message_count: 1,
  participants: [`Sender ${i} <s${i}@example.co.jp>`],
  received_count: 1,
  snippet: 'ご確認ください。',
  subject: `Subject ${i}`,
  thread_id: `t${i}`,
  // read and unread both picked, so the menu is there at every width
  unread_count: i % 2,
}))

const ACTION_COUNT = 5

/** The list header's toggle, by its tooltip. */
const BATCH_SELECT: Record<string, string> = { en: 'Batch select', ja: '一括選択', zh: '批量选择' }

async function pickRows(page: Page, lang: string, count: number) {
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    let json: unknown = {}
    if (/categories/.test(path)) json = []
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
  const rows = page.locator('[role="listitem"]')
  await rows.first().waitFor()
  await page.getByTitle(BATCH_SELECT[lang]).click()
  for (let i = 0; i < count; i++) {
    const box = await rows.nth(i).boundingBox()
    if (!box) throw new Error(`no row ${i}`)
    await page.mouse.click(box.x + 80, box.y + box.height / 2)
  }
  await page.locator('[data-batch-bar]').waitFor()
}

/** The bar's box, and every control in it. */
async function measure(page: Page) {
  return page.locator('[data-batch-bar]').evaluate((bar) => {
    const box = bar.getBoundingClientRect()
    const controls = Array.from(bar.querySelectorAll('button')).map((b) => {
      const r = b.getBoundingClientRect()
      return { bottom: r.bottom, left: r.left, right: r.right, top: r.top }
    })
    return {
      bar: {
        bottom: box.bottom,
        height: box.height,
        left: box.left,
        right: box.right,
        top: box.top,
      },
      clientWidth: bar.clientWidth,
      controls,
      inline: bar.querySelectorAll('[data-batch-actions] > [data-batch-action]').length,
      scrollWidth: bar.scrollWidth,
    }
  })
}

const WIDTHS = [
  // a phone: the list is the whole 320 px
  { height: 700, name: '320', width: 320 },
  // the desktop list pane
  { height: 900, name: 'desktop pane', width: 1440 },
]

for (const size of WIDTHS) {
  for (const lang of ['zh', 'ja', 'en']) {
    test(`one row, nothing cut off: ${size.name}, ${lang}`, async ({ page }) => {
      await page.setViewportSize({ height: size.height, width: size.width })
      await pickRows(page, lang, 4)
      const m = await measure(page)
      expect(m.bar.height).toBe(48)
      expect(m.scrollWidth).toBeLessThanOrEqual(m.clientWidth)
      for (const c of m.controls) {
        expect(c.top).toBeGreaterThanOrEqual(m.bar.top)
        expect(c.bottom).toBeLessThanOrEqual(m.bar.bottom)
        expect(c.left).toBeGreaterThanOrEqual(m.bar.left)
        expect(c.right).toBeLessThanOrEqual(m.bar.right)
      }
      // no two controls share a column
      const sorted = [...m.controls].sort((a, b) => a.left - b.left)
      for (let i = 1; i < sorted.length; i++) {
        expect(sorted[i].left).toBeGreaterThanOrEqual(sorted[i - 1].right)
      }
    })
  }
}

test('at 320 px the actions that do not fit are in the menu, none lost', async ({ page }) => {
  await page.setViewportSize({ height: 700, width: 320 })
  await pickRows(page, 'zh', 4)
  const m = await measure(page)
  expect(m.inline).toBeGreaterThan(0)
  expect(m.inline).toBeLessThan(ACTION_COUNT)
  await page.locator('[data-batch-bar] [aria-haspopup="menu"]').click()
  const inMenu = await page.locator('[role="menu"] [data-batch-action]').count()
  // the menu also holds "unread", which the mixed selection adds
  expect(m.inline + inMenu).toBe(ACTION_COUNT + 1)
  const menu = await page.locator('[role="menu"]').boundingBox()
  if (!menu) throw new Error('the menu has no box')
  expect(menu.x).toBeGreaterThanOrEqual(0)
  expect(menu.x + menu.width).toBeLessThanOrEqual(320)
})

test('the desktop pane draws every action inline', async ({ page }) => {
  await page.setViewportSize({ height: 900, width: 1440 })
  await pickRows(page, 'zh', 4)
  expect((await measure(page)).inline).toBe(ACTION_COUNT)
})

test('the box sits in the column of the rows’ boxes', async ({ page }) => {
  await page.setViewportSize({ height: 900, width: 1440 })
  await pickRows(page, 'zh', 2)
  const all = await page.locator('[data-batch-bar] [role="checkbox"]').boundingBox()
  const row = await page.locator('[role="listitem"] [data-row-dims]').first().boundingBox()
  if (!all || !row) throw new Error('a checkbox has no box')
  expect(all.x).toBe(row.x)
})
