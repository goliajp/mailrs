import type { Page, WebSocketRoute } from '@playwright/test'

import { expect, test } from '@playwright/test'

/**
 * New mail shows up in the list that is open, and in the list you switch
 * to, without a reload.
 *
 * Driven through the real app with the API and the `/api/events` socket
 * stubbed: the stub server holds a list per tab and grows them when a
 * message "arrives", then announces it the way webapi does.
 */

const USER = 'a@golia.jp'
const NOW = Math.floor(Date.now() / 1000)

type Row = { participants: string[]; subject: string; thread_id: string }

function row(r: Row, at: number, category: string) {
  return {
    category,
    last_date: at,
    message_count: 1,
    participants: r.participants,
    received_count: 1,
    snippet: r.subject,
    subject: r.subject,
    thread_id: r.thread_id,
    unread_count: 1,
  }
}

type Server = {
  announce: (r: Row) => void
  deliver: (folder: 'Inbox' | 'NP', r: Row) => void
}

async function stub(page: Page): Promise<Server> {
  const lists: Record<string, ReturnType<typeof row>[]> = {
    Inbox: [
      row(
        { participants: ['old@x.jp'], subject: 'older inbox mail', thread_id: 'old-inbox' },
        NOW - 3600,
        'inbox'
      ),
    ],
    NP: [
      row(
        { participants: ['old@np.jp'], subject: 'older notification', thread_id: 'old-np' },
        NOW - 3600,
        'notification'
      ),
    ],
  }
  let socket: null | WebSocketRoute = null
  await page.routeWebSocket('**/api/events**', (ws) => {
    socket = ws
  })
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    let json: unknown = {}
    if (/categories/.test(path)) json = []
    else if (/\/conversations$/.test(path)) {
      const folder = url.searchParams.get('folder') ?? 'Inbox'
      const unread = url.searchParams.get('unread') === 'true'
      let items = folder === 'NonJunk' ? [...lists.Inbox, ...lists.NP] : (lists[folder] ?? [])
      if (unread) items = items.filter((i) => i.unread_count > 0)
      items = [...items].sort((a, b) => b.last_date - a.last_date)
      json = { has_more: false, items, next_cursor: null }
    } else if (/\/conversations\/[^/]+$/.test(path)) json = []
    await route.fulfill({ json, status: 200 })
  })
  await page.addInitScript((user) => {
    localStorage.setItem(
      'mailrs_auth',
      JSON.stringify({
        accessible_domains: ['golia.jp'],
        address: user,
        display_name: 'A',
        permissions: [],
        token: 'stub',
      })
    )
  }, USER)
  let tick = 0
  return {
    announce: (r) => {
      socket?.send(
        JSON.stringify({
          sender: r.participants[0],
          snippet: '',
          subject: r.subject,
          thread_id: r.thread_id,
          type: 'NewMessage',
          user: USER,
        })
      )
    },
    deliver: (folder, r) => {
      tick += 1
      lists[folder] = [
        row(r, NOW + tick, folder === 'NP' ? 'notification' : 'inbox'),
        ...lists[folder],
      ]
    },
  }
}

const rows = (page: Page) => page.locator('[role="listitem"]')

async function openTab(page: Page, label: string) {
  await page.getByRole('button', { exact: true, name: label }).first().click()
}

test.use({ viewport: { height: 900, width: 1400 } })

test('mail for the open tab appears in it', async ({ page }) => {
  const server = await stub(page)
  await page.goto('/mail')
  await expect(rows(page)).toHaveCount(1)
  const fresh = { participants: ['new@x.jp'], subject: 'fresh inbox mail', thread_id: 'new-inbox' }
  server.deliver('Inbox', fresh)
  server.announce(fresh)
  await expect(rows(page).filter({ hasText: fresh.subject })).toHaveCount(1)
})

test('mail for another tab is there when you switch to it', async ({ page }) => {
  const server = await stub(page)
  await page.goto('/mail')
  await expect(rows(page)).toHaveCount(1)
  // N & P visited once, so it has a cached list of its own
  await openTab(page, 'N & P')
  await expect(rows(page).filter({ hasText: 'older notification' })).toHaveCount(1)
  await openTab(page, 'Inbox')
  await expect(rows(page).filter({ hasText: 'older inbox mail' })).toHaveCount(1)

  const note = { participants: ['noreply@freee.work'], subject: 'card used', thread_id: 'new-np' }
  server.deliver('NP', note)
  server.announce(note)
  await page.waitForTimeout(500)
  await openTab(page, 'Unread')
  await expect(rows(page).filter({ hasText: note.subject })).toHaveCount(1)
  await openTab(page, 'N & P')
  await expect(rows(page).filter({ hasText: note.subject })).toHaveCount(1)
})

test('mail for the open tab appears in it, for N & P too', async ({ page }) => {
  const server = await stub(page)
  await page.goto('/mail')
  await openTab(page, 'N & P')
  await expect(rows(page).filter({ hasText: 'older notification' })).toHaveCount(1)
  const note = {
    participants: ['noreply@freee.work'],
    subject: 'receipt report',
    thread_id: 'new-np-2',
  }
  server.deliver('NP', note)
  server.announce(note)
  await expect(rows(page).filter({ hasText: note.subject })).toHaveCount(1)
})
