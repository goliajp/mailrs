import type { Page } from '@playwright/test'

import { expect, test } from '@playwright/test'

/**
 * The desktop shell in both themes: the icon rail on a darker band, then
 * the panes edge to edge with a hairline between them, the mail page's
 * folder column first. Measured in a browser, because jsdom does no layout
 * and computes no colours.
 */

/** WCAG contrast ratio of two computed `rgb(...)` colours. */
function contrast(a: string, b: string): number {
  const [la, lb] = [luminance(a), luminance(b)]
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
}

function luminance(rgb: string): number {
  const channels = (rgb.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number)
  if (channels.length !== 3) throw new Error(`not an rgb colour: ${rgb}`)
  const [r, g, b] = channels.map((v) => {
    const c = v / 255
    if (c <= 0.04045) return c / 12.92
    return ((c + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

async function openMail(page: Page, items: unknown[] = []) {
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    let json: unknown = {}
    if (/categories/.test(path)) json = []
    else if (/conversations$/.test(path)) {
      json = { has_more: false, items, next_cursor: null }
    }
    await route.fulfill({ json, status: 200 })
  })
  await page.addInitScript(() => {
    localStorage.setItem('mailrs_lang', 'en')
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
  await page.goto('/mail')
  await page.locator('aside').waitFor()
  await page.locator('[data-component="panel"]').first().waitFor()
}

test.use({ viewport: { height: 900, width: 1440 } })

for (const colorScheme of ['light', 'dark'] as const) {
  test.describe(colorScheme, () => {
    test.use({ colorScheme })

    test('the rail is a darker band and its icons sit in its middle', async ({ page }) => {
      await openMail(page)
      const look = await page.evaluate(() => {
        const rail = document.querySelector('aside')
        const pane = document.querySelector('[data-component="panel"]')
        if (!rail || !pane) throw new Error('rail or pane missing')
        const box = rail.getBoundingClientRect()
        return {
          middle: box.left + box.width / 2,
          paneBg: getComputedStyle(pane).backgroundColor,
          // the rail is transparent; the band is the shell behind it
          railBg: getComputedStyle(rail.closest('[data-component="app-shell"]') as Element)
            .backgroundColor,
        }
      })
      expect(contrast(look.railBg, look.paneBg)).toBeGreaterThan(1.08)

      const centres = await page
        .locator('aside')
        .locator('a, button')
        .evaluateAll((els) =>
          els.map((el) => {
            const box = el.getBoundingClientRect()
            return box.left + box.width / 2
          })
        )
      expect(centres.length).toBeGreaterThan(3)
      for (const c of centres) expect(Math.abs(c - look.middle)).toBeLessThanOrEqual(0.5)
    })

    test('the panes run edge to edge with a hairline between them', async ({ page }) => {
      await openMail(page)
      const look = await page.evaluate(() => {
        const probe = document.createElement('div')
        probe.className = 'border-border border'
        document.body.append(probe)
        const token = getComputedStyle(probe).borderTopColor
        probe.remove()
        const rail = document.querySelector('aside')
        const panes = [...document.querySelectorAll('[data-component="panel"]')]
          .filter((el) => el.getBoundingClientRect().width > 0)
          .sort((a, b) => a.getBoundingClientRect().left - b.getBoundingClientRect().left)
        if (!rail || panes.length < 3) throw new Error('rail or panes missing')
        return {
          edges: panes.map((el) => {
            const r = el.getBoundingClientRect()
            const st = getComputedStyle(el)
            return {
              left: r.left,
              line: st.borderRightColor,
              lineWidth: parseFloat(st.borderRightWidth),
              radius: parseFloat(st.borderTopLeftRadius),
              right: r.right,
            }
          }),
          railRight: rail.getBoundingClientRect().right,
          token,
        }
      })
      // folders, list, reader: the first starts where the rail ends
      expect(look.edges[0].left).toBe(look.railRight)
      for (const [i, e] of look.edges.entries()) {
        expect(e.radius).toBe(0)
        if (i === 0) continue
        expect(e.left).toBe(look.edges[i - 1].right)
        expect(look.edges[i - 1].lineWidth).toBe(1)
        expect(look.edges[i - 1].line).toBe(look.token)
      }
    })

    test('the folder column collapses into the tab chips and comes back', async ({ page }) => {
      await openMail(page)
      const column = page.getByRole('heading', { name: 'Mail', exact: true })
      // the folder column's own tablist sits directly in its <nav>; the
      // chips on top of the list do not
      const chips = page.locator(':not(nav) > [role="tablist"][aria-label="Mail lists"]')
      await expect(column).toBeVisible()
      await expect(chips).toBeHidden()

      await page.getByRole('button', { name: 'Hide folders' }).click()
      await expect(column).toBeHidden()
      // the tabs the column stood in for are back on top of the list
      await expect(chips).toBeVisible()
      await expect(page.getByRole('button', { name: 'Show folders' })).toBeVisible()

      // kept per browser
      await page.reload()
      await expect(page.getByRole('button', { name: 'Show folders' })).toBeVisible()
      await expect(column).toBeHidden()

      await page.getByRole('button', { name: 'Show folders' }).click()
      await expect(column).toBeVisible()
      await expect(chips).toBeHidden()
      await expect(page.getByRole('button', { name: 'Show folders' })).toBeHidden()
    })

    test('the top rows of the panes end on one line', async ({ page }) => {
      await openMail(
        page,
        ['t1', 't2'].map((thread_id, i) => ({
          last_date: 1_793_900_000 - i * 4000,
          message_count: 1,
          participants: [`s${i}@example.com`],
          received_count: 1,
          snippet: 's',
          subject: `Thread ${i + 1}`,
          thread_id,
          unread_count: 0,
        }))
      )
      await page
        .getByRole('button', { name: /^Send$/ })
        .first()
        .waitFor()
      const bottoms = await page.evaluate(() => {
        const row = (el: Element | null | undefined) => {
          if (!el) throw new Error('top row missing')
          return el.getBoundingClientRect().bottom
        }
        return {
          folders: row(document.querySelector('nav h2')?.parentElement),
          list: row(document.querySelector('[role="search"]')?.closest('.border-b')),
          // the conversation's subject, the one heading outside the folder column
          reading: row(
            [...document.querySelectorAll('h2')]
              .find((h) => !h.closest('nav'))
              ?.closest('.border-b')
          ),
        }
      })
      expect(bottoms.list).toBe(bottoms.folders)
      expect(bottoms.reading).toBe(bottoms.folders)
    })

    test('dividers inside a pane can be seen', async ({ page }) => {
      await openMail(page)
      const look = await page.evaluate(() => {
        const probe = document.createElement('div')
        probe.className = 'border-border border'
        document.body.append(probe)
        const line = getComputedStyle(probe).borderTopColor
        probe.remove()
        const pane = document.querySelector('[data-component="panel"]')
        if (!pane) throw new Error('pane missing')
        return { line, pane: getComputedStyle(pane).backgroundColor }
      })
      // zinc-200 on a white pane was 1.26:1 and the lines between a pane's
      // rows disappeared in light mode
      const floor = colorScheme === 'light' ? 1.4 : 1.1
      expect(contrast(look.line, look.pane)).toBeGreaterThan(floor)
    })

    test('text on the accent colour is readable wherever it is drawn', async ({ page }) => {
      // a conversation open, so the reply box and its send button are on
      // screen, and one unread, so the unread count is too
      await openMail(
        page,
        ['t1', 't2'].map((thread_id, i) => ({
          last_date: 1_793_900_000 - i * 4000,
          message_count: 1,
          participants: [`s${i}@example.com`],
          received_count: 1,
          snippet: 's',
          subject: `Thread ${i + 1}`,
          thread_id,
          unread_count: i,
        }))
      )
      await page
        .getByRole('button', { name: /^Send$/ })
        .first()
        .waitFor()
      const pairs = await page.evaluate(() => {
        const probe = document.createElement('span')
        probe.className = 'bg-accent'
        document.body.append(probe)
        const accent = getComputedStyle(probe).backgroundColor
        probe.remove()
        // every element on the page painted in the accent that holds text
        // or an icon, the send button among them
        return [...document.querySelectorAll('body *')]
          .filter((el) => getComputedStyle(el).backgroundColor === accent)
          .filter((el) => (el.textContent ?? '').trim() !== '' || el.querySelector('svg'))
          .map((el) => ({
            bg: accent,
            fg: getComputedStyle(el).color,
            what: el.outerHTML.slice(0, 80),
          }))
      })
      expect(pairs.length).toBeGreaterThan(0)
      for (const p of pairs) {
        // white on the dark accent was 3.68:1; WCAG AA for body text is 4.5:1
        expect(contrast(p.fg, p.bg), p.what).toBeGreaterThanOrEqual(4.5)
      }
    })
  })
}
