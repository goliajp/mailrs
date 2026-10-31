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
