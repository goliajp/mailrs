import type { Page } from '@playwright/test'

import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

/**
 * No serious or critical accessibility problem on the screens a reader
 * uses every day, by axe-core, against the real app with the API stubbed.
 * Each screen is checked to have its content on screen before it is
 * scanned: an empty screen has nothing to fail.
 */

const MESSAGES = [
  {
    attachments: [{ filename: 'invoice.pdf', index: 0, size: 12345 }],
    html_body: '<p>Please find the invoice attached.</p>',
    internal_date: 1754400000,
    recipients: 'a@golia.jp',
    sender: 'billing@example.com',
    subject: 'Invoice for October',
    text_body: 'Please find the invoice attached.',
    uid: 1,
  },
]

const THREADS = [
  {
    last_date: 1754400000,
    message_count: 1,
    participants: ['billing@example.com'],
    received_count: 1,
    snippet: 'Please find the invoice attached.',
    subject: 'Invoice for October',
    thread_id: 't1',
    unread_count: 1,
  },
  {
    last_date: 1754300000,
    message_count: 2,
    participants: ['colleague@golia.jp'],
    received_count: 2,
    snippet: 'Meeting notes',
    subject: 'Weekly sync',
    thread_id: 't2',
    unread_count: 0,
  },
]

async function seriousProblems(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).analyze()
  return result.violations
    .filter((v) => v.impact === 'serious' || v.impact === 'critical')
    .map(
      (v) =>
        `${v.impact} ${v.id}: ${v.help} — ${v.nodes
          .map((n) => `${n.target.join(' ')} [${n.any[0]?.message ?? ''}]`)
          .slice(0, 5)
          .join(' | ')}`
    )
}

async function stubApi(page: Page, admin = false) {
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    let json: unknown = {}
    if (/categories/.test(path)) json = []
    else if (/conversations\/t1$/.test(path)) json = MESSAGES
    else if (/conversations$/.test(path))
      json = { has_more: false, items: THREADS, next_cursor: null }
    else if (/auth\/sessions$|auth\/app-passwords$|labels$|saved-searches$|rules$/.test(path))
      json = []
    await route.fulfill({ json, status: 200 })
  })
  await page.addInitScript((isAdmin) => {
    localStorage.setItem(
      'mailrs_auth',
      JSON.stringify({
        accessible_domains: ['golia.jp'],
        address: 'a@golia.jp',
        display_name: 'A',
        is_admin: isAdmin,
        is_super: isAdmin,
        permissions: [],
        token: 'stub',
      })
    )
  }, admin)
}

const SCREENS: { name: string; path: string; ready: string }[] = [
  { name: 'inbox', path: '/', ready: 'Invoice for October' },
  { name: 'settings: account', path: '/settings', ready: 'Change Password' },
  { name: 'settings: security', path: '/settings?tab=security', ready: 'App passwords' },
]

for (const screen of SCREENS) {
  test(`no serious accessibility problem: ${screen.name}`, async ({ page }) => {
    await stubApi(page)
    await page.goto(screen.path)
    await expect(page.getByText(screen.ready).first()).toBeVisible()
    expect(await seriousProblems(page)).toEqual([])
  })
}

test('no serious accessibility problem: an open conversation', async ({ page }) => {
  await stubApi(page)
  await page.goto('/')
  await page.getByText('Invoice for October').first().click()
  await expect(page.getByText('Please find the invoice attached.').first()).toBeVisible()
  expect(await seriousProblems(page)).toEqual([])
})

const ADMIN_SCREENS: { name: string; path: string; ready: string }[] = [
  { name: 'admin: protocol sign-ins', path: '/admin/protocol-sign-ins', ready: 'a@golia.jp' },
  { name: 'admin: screening', path: '/admin/screening', ready: 'Addresses and domains' },
]

for (const screen of ADMIN_SCREENS) {
  test(`no serious accessibility problem: ${screen.name}`, async ({ page }) => {
    await stubApi(page, true)
    await page.goto(screen.path)
    await expect(page.getByText(screen.ready).first()).toBeVisible()
    expect(await seriousProblems(page)).toEqual([])
  })
}

test('no serious accessibility problem: writing a message', async ({ page }) => {
  await stubApi(page)
  await page.goto('/')
  await expect(page.getByText('Invoice for October').first()).toBeVisible()
  await page.locator('[aria-keyshortcuts="C"]').first().click()
  await expect(page.getByPlaceholder('recipient@example.com').first()).toBeVisible()
  expect(await seriousProblems(page)).toEqual([])
})

test('the mail list tabs are a tab list the arrow keys move through', async ({ page }) => {
  await stubApi(page)
  await page.goto('/mail')
  const tabs = page.getByRole('tablist', { name: 'Mail lists' }).getByRole('tab')
  await expect(tabs.first()).toBeVisible()
  const selected = page.getByRole('tab', { selected: true })
  await expect(selected).toHaveCount(1)
  await selected.focus()
  const before = await selected.textContent()
  await page.keyboard.press('ArrowRight')
  const focused = await page.evaluate(() => document.activeElement?.getAttribute('role'))
  expect(focused).toBe('tab')
  expect(await page.evaluate(() => document.activeElement?.textContent)).not.toBe(before)
})

// Who sees the admin entry, at the four widths the layout changes at: the
// phone's bottom bar, the tablet, the narrow desktop, the wide desktop.
const WIDTHS = [390, 768, 1024, 1440]

async function signedInAs(page: Page, who: { is_admin: boolean; permissions: string[] }) {
  await page.route('**/api/**', (route) => route.fulfill({ json: [], status: 200 }))
  await page.addInitScript((w) => {
    localStorage.setItem(
      'mailrs_auth',
      JSON.stringify({
        accessible_domains: ['golia.jp'],
        address: 'a@golia.jp',
        display_name: 'A',
        is_super: false,
        token: 'stub',
        ...w,
      })
    )
  }, who)
}

for (const width of WIDTHS) {
  test(`at ${width}px an account without admin permissions has no admin entry`, async ({
    page,
  }) => {
    await page.setViewportSize({ height: 800, width })
    await signedInAs(page, { is_admin: false, permissions: [] })
    await page.goto('/settings')
    await expect(page.getByRole('link', { name: 'Settings' }).first()).toBeVisible()
    await expect(page.locator('a[href="/admin"]')).toHaveCount(0)
  })

  test(`at ${width}px a partial administrator has the admin entry`, async ({ page }) => {
    await page.setViewportSize({ height: 800, width })
    await signedInAs(page, { is_admin: true, permissions: ['admin.aliases'] })
    await page.goto('/settings')
    await expect(page.locator('a[href="/admin"]').first()).toBeVisible()
  })
}

// Seven messages, the fourth unread: the newest and the unread one open,
// the second and third fold into one line.
const LONG_CONVERSATION = Array.from({ length: 7 }, (_, i) => ({
  attachments: [],
  flags: i === 3 ? 0 : 1,
  html_body: null,
  internal_date: 1754400000 + i * 3600,
  recipients: 'a@golia.jp',
  sender: `Person ${i + 1} <p${i + 1}@example.com>`,
  subject: 'Invoice for October',
  text_body: `Message number ${i + 1}`,
  uid: i + 1,
}))

test('a long conversation reads as a stack the keyboard moves through', async ({ page }) => {
  await stubApi(page)
  await page.route('**/api/conversations/t1', (route) =>
    route.fulfill({ json: LONG_CONVERSATION, status: 200 })
  )
  await page.goto('/mail')
  const stack = page.getByRole('group', { name: 'Messages in this conversation' })
  const headers = stack.locator('[data-stack-header]')
  await expect(headers.first()).toBeVisible()
  await expect(stack.locator('[data-stack-header][aria-expanded="true"]')).toHaveCount(2)
  await expect(stack.getByRole('button', { name: 'Show 2 earlier messages' })).toBeVisible()
  expect(await seriousProblems(page)).toEqual([])

  const five = headers.filter({ hasText: 'Person 5' })
  await expect(five).toHaveAttribute('aria-expanded', 'false')
  await five.click()
  await expect(five).toHaveAttribute('aria-expanded', 'true')

  await headers.first().focus()
  await page.keyboard.press('ArrowDown')
  await expect(headers.filter({ hasText: 'Person 4' })).toBeFocused()
  await page.keyboard.press('j')
  await expect(five).toBeFocused()
  await page.keyboard.press('o')
  await expect(five).toHaveAttribute('aria-expanded', 'false')
  // the keys moved within the conversation, not the list to another one
  await expect(headers.first()).toBeVisible()
})
