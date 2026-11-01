import type { Page } from '@playwright/test'

import { expect, test } from '@playwright/test'

/**
 * The keyboard, end to end in the real app with the API stubbed: moving
 * through the list, and the action keys reaching the endpoints the action
 * table names for them — including `z` taking the last one back.
 */

const THREADS = ['t1', 't2', 't3'].map((id, i) => ({
  last_date: 1754400000 - i * 1000,
  message_count: 1,
  participants: [`sender${i}@example.com`],
  received_count: 1,
  snippet: `snippet ${i}`,
  subject: `Subject ${i + 1}`,
  thread_id: id,
  unread_count: 0,
}))

async function app(page: Page): Promise<string[]> {
  const calls: string[] = []
  await page.route('**/api/**', async (route) => {
    const req = route.request()
    const path = new URL(req.url()).pathname
    if (req.method() !== 'GET') calls.push(`${req.method()} ${path}`)
    let json: unknown = {}
    if (/conversations$/.test(path) && req.method() === 'GET')
      json = { has_more: false, items: THREADS, next_cursor: null }
    else if (/categories|labels$|saved-searches$|rules$|sessions$/.test(path)) json = []
    else if (/conversations\/t\d$/.test(path) && req.method() === 'GET') json = [message(path)]
    // the action endpoints answer 204, as the server does
    if (req.method() !== 'GET') {
      await route.fulfill({ body: '', status: 204 })
      return
    }
    await route.fulfill({ json, status: 200 })
  })
  await page.addInitScript(() => {
    localStorage.setItem(
      'mailrs_auth',
      JSON.stringify({
        accessible_domains: ['example.com'],
        address: 'a@example.com',
        display_name: 'A',
        is_admin: false,
        is_super: false,
        permissions: [],
        token: 'stub',
      })
    )
  })
  await page.setViewportSize({ height: 900, width: 1440 })
  await page.goto('/mail')
  await expect(current(page)).toHaveAccessibleName(/Subject 1/)
  // the reading pane's actions are there once the open conversation's row is
  await expect(page.getByRole('button', { name: 'Archive' }).first()).toBeVisible()
  // keys go to the page, not to a field that happens to hold focus
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  return calls
}

function message(path: string) {
  const n = Number(path.slice(-1))
  return {
    attachments: [],
    html_body: '',
    internal_date: 1754400000,
    recipients: 'a@example.com',
    sender: `sender${n - 1}@example.com`,
    subject: `Subject ${n}`,
    text_body: `body ${n}`,
    uid: n,
  }
}

const current = (page: Page) => page.locator('[aria-current="true"]').first()

test('j and k move through the list', async ({ page }) => {
  await app(page)
  await page.keyboard.press('j')
  await expect(current(page)).toHaveAccessibleName(/Subject 2/)
  await page.keyboard.press('j')
  await expect(current(page)).toHaveAccessibleName(/Subject 3/)
  await page.keyboard.press('k')
  await expect(current(page)).toHaveAccessibleName(/Subject 2/)
})

test('s stars, e archives and z takes the archive back', async ({ page }) => {
  const calls = await app(page)
  await page.keyboard.press('s')
  await expect.poll(() => calls).toContain('POST /api/conversations/t1/star')
  await page.keyboard.press('e')
  await expect.poll(() => calls).toContain('POST /api/conversations/t1/archive')
  // what can be taken back is known once the archive has succeeded
  await expect(page.getByRole('button', { name: 'Undo' })).toBeVisible()
  await page.keyboard.press('z')
  await expect.poll(() => calls).toContain('POST /api/conversations/t1/unarchive')
})

test('# moves the open conversation to the trash', async ({ page }) => {
  const calls = await app(page)
  await page.keyboard.press('#')
  await expect.poll(() => calls).toContain('DELETE /api/conversations/t1')
})

test('x selects for a batch action', async ({ page }) => {
  await app(page)
  await page.keyboard.press('x')
  await expect(page.getByText('1 selected')).toBeVisible()
})

test('? shows the shortcuts and Escape closes them', async ({ page }) => {
  await app(page)
  await page.keyboard.press('?')
  await expect(page.getByText('Keyboard Shortcuts')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByText('Keyboard Shortcuts')).toBeHidden()
})
