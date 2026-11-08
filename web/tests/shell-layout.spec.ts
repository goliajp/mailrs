import type { Page } from '@playwright/test'

import { expect, test } from '@playwright/test'

/**
 * The desktop shell in both themes: the icon rail and the floating panes.
 *
 * In light the shell background was #fafafa behind #ffffff panes, so the
 * gaps and rounded corners that read as floating panes in dark could not
 * be seen, and the rail looked off-centre in a 68 px blank column. Measured
 * in a browser, because jsdom does no layout and computes no colours.
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

    test('the rail has a divider on its right and its icons sit in its middle', async ({
      page,
    }) => {
      await openMail(page)
      const rail = await page.locator('aside').evaluate((el) => {
        const style = getComputedStyle(el)
        const probe = document.createElement('div')
        probe.className = 'border-border border'
        document.body.append(probe)
        const token = getComputedStyle(probe).borderTopColor
        probe.remove()
        const box = el.getBoundingClientRect()
        const border = parseFloat(style.borderRightWidth)
        return {
          border,
          borderColor: style.borderRightColor,
          borderStyle: style.borderRightStyle,
          middle: box.left + (box.width - border) / 2,
          right: box.right,
          token,
        }
      })
      expect(rail.border).toBe(1)
      expect(rail.borderStyle).toBe('solid')
      expect(rail.borderColor).toBe(rail.token)

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
      for (const c of centres) expect(Math.abs(c - rail.middle)).toBeLessThanOrEqual(0.5)
    })

    test('the panes stand out from the shell behind them', async ({ page }) => {
      await openMail(page)
      const look = await page.evaluate(() => {
        const shell = document.querySelector('[data-component="app-shell"]')
        const rail = document.querySelector('aside')
        const panes = [...document.querySelectorAll('[data-component="panel"]')]
          .map((el) => el.getBoundingClientRect())
          .filter((r) => r.width > 0)
          .sort((a, b) => a.left - b.left)
        const pane = document.querySelector('[data-component="panel"]')
        if (!shell || !rail || !pane) throw new Error('shell, rail or pane missing')
        return {
          firstPaneLeft: panes[0].left,
          firstPaneRight: panes[0].right,
          paneBg: getComputedStyle(pane).backgroundColor,
          railRight: rail.getBoundingClientRect().right,
          secondPaneLeft: panes[1].left,
          shellBg: getComputedStyle(shell).backgroundColor,
        }
      })
      // light was 1.04 (#fafafa behind #ffffff), dark 1.12; the panes only
      // read as panes when the shell is as far from them as in dark
      expect(contrast(look.shellBg, look.paneBg)).toBeGreaterThan(1.08)
      // the rail's divider is as far from the first pane as one pane is from the next
      const paneGap = look.secondPaneLeft - look.firstPaneRight
      expect(paneGap).toBeGreaterThan(0)
      expect(look.firstPaneLeft - look.railRight).toBeCloseTo(paneGap, 0)
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
