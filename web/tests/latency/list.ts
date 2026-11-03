/**
 * Reading the mail list from the page: which conversation rows are
 * rendered, and moving its scroll position; and the renderer's main-thread
 * time so far.
 */

import type { CDPSession, Page } from '@playwright/test'

export type RowInfo = {
  current: boolean
  index: number
  n: number
  unread: boolean
  visible: boolean
}

/** The rendered rows, in list order. */
export function rows(page: Page): Promise<RowInfo[]> {
  return page.evaluate(() => {
    const list = document.querySelector('[role="list"][aria-label]')
    const box = list?.getBoundingClientRect()
    const out: RowInfo[] = []
    for (const slot of Array.from(document.querySelectorAll<HTMLElement>('[data-index]'))) {
      const b = slot.querySelector<HTMLButtonElement>('[role="listitem"] > button')
      if (!b) continue
      const label = b.getAttribute('aria-label') ?? ''
      const m = /#(\d+)(?:,|$)/.exec(label)
      if (!m) continue
      const r = slot.getBoundingClientRect()
      out.push({
        current: b.getAttribute('aria-current') === 'true',
        index: Number(slot.dataset.index),
        n: Number(m[1]),
        unread: /, \d+ unread/.test(label),
        visible: !!box && r.top >= box.top && r.bottom <= box.bottom,
      })
    }
    return out.sort((a, b) => a.index - b.index)
  })
}

export const rowButton = (page: Page, n: number) =>
  page.getByRole('button', { exact: false, name: new RegExp(`#${n}(,|$)`) })

/** Main-thread time so far, ms, by kind. */
export async function mainThread(cdp: CDPSession) {
  const { metrics } = await cdp.send('Performance.getMetrics')
  const get = (n: string) => (metrics.find((m) => m.name === n)?.value ?? 0) * 1000
  return {
    layout: get('LayoutDuration'),
    script: get('ScriptDuration'),
    style: get('RecalcStyleDuration'),
    task: get('TaskDuration'),
  }
}

export async function scrollList(page: Page, to: 'bottom' | number) {
  await page.evaluate((t) => {
    const list = document.querySelector<HTMLElement>('[role="list"][aria-label]')
    if (!list) return
    if (t === 'bottom') {
      list.scrollTop = list.scrollHeight
      return
    }
    list.scrollTop = list.scrollHeight - list.clientHeight - t
  }, to)
}
