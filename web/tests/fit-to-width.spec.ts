import { expect, test } from '@playwright/test'

/**
 * Fit-to-width, measured in a browser that does layout.
 *
 * This cannot be a vitest: jsdom reports `scrollWidth: 0` for everything,
 * so `fitScale` would be handed a zero, answer 1, and the assertion would
 * pass without a transform ever being applied. The unit test covers the
 * arithmetic; this covers whether the arithmetic reaches the pixels.
 *
 * It earned its place immediately — the first implementation set an inline
 * `width` that the stylesheet's `max-width: 680px` silently overrode, so
 * the wrap's box stayed 680 while its content painted out to 786. Nothing
 * but a measurement would have caught it.
 *
 * The fixture renders the real `HtmlFrame` through vite's dev server, so
 * what is under test is the component, not a copy of it.
 */

const PHONE = { height: 800, width: 390 }
// 390 viewport less the fixture column's padding — the width a message
// actually gets on a phone.
const COLUMN = 366

type Measured = {
  boxWidth: number
  columnWidth: number
  scale: number
  scrollWidth: number
}

async function measure(page: import('@playwright/test').Page): Promise<Measured> {
  return page.evaluate(() => {
    const host = document.querySelector('#col > div') as HTMLElement
    const wrap = host.shadowRoot?.querySelector('.mail-wrap') as HTMLElement
    const m = /scale\(([\d.]+)\)/.exec(wrap.style.transform)
    return {
      boxWidth: Math.round(wrap.getBoundingClientRect().width),
      columnWidth: host.clientWidth,
      scale: m ? Number(m[1]) : 1,
      scrollWidth: host.scrollWidth,
    }
  })
}

async function open(page: import('@playwright/test').Page, width: number) {
  await page.goto(`/tests/fixtures/fit.html?w=${width}`)
  await page.waitForFunction(() => {
    const host = document.querySelector('#col > div') as HTMLElement | null
    return !!host?.shadowRoot?.querySelector('.mail-wrap')
  })
  // One frame for the ResizeObserver pass that follows first paint.
  await page.waitForTimeout(300)
}

test.use({ viewport: PHONE })

/**
 * The widths a survey of 400 real messages found. Every one of them has
 * to land exactly on the column — that is the whole feature.
 */
for (const width of [400, 600, 700, 768]) {
  test(`a ${width}px email fills the phone column and nothing spills`, async ({ page }) => {
    await open(page, width)
    const m = await measure(page)
    expect(m.columnWidth).toBe(COLUMN)
    expect(m.scale).toBeLessThan(1)
    expect(m.boxWidth).toBe(COLUMN)
    // Nothing left over means nothing was clipped away, which is the
    // state this replaced: `overflow-hidden` and the rest simply gone.
    expect(m.scrollWidth).toBe(COLUMN)
  })
}

test('a pathological width stops at the floor and stays reachable', async ({ page }) => {
  await open(page, 3000)
  const m = await measure(page)
  expect(m.scale).toBeCloseTo(0.45, 3)
  // Scaled far enough to be readable, not far enough to fit — so the
  // remainder must scroll rather than disappear.
  expect(m.scrollWidth).toBeGreaterThan(m.columnWidth)
})

test('growing the column back to full size undoes every property fit wrote', async ({ page }) => {
  await open(page, 600)
  expect((await measure(page)).scale).toBeLessThan(1)

  await page.evaluate(() => {
    ;(document.getElementById('col') as HTMLElement).style.width = '900px'
  })
  await page.waitForTimeout(300)
  const wide = await measure(page)
  expect(wide.scale).toBe(1)
  // `max-width: 680px` is back and the auto margins centre it again —
  // an earlier version cleared the transform but left `margin-left: 0`,
  // and the email stayed pinned to the left of a wide pane.
  expect(wide.boxWidth).toBe(680)
  const centred = await page.evaluate(() => {
    const host = document.querySelector('#col > div') as HTMLElement
    const wrap = host.shadowRoot?.querySelector('.mail-wrap') as HTMLElement
    const h = host.getBoundingClientRect()
    const w = wrap.getBoundingClientRect()
    return Math.round(w.left - h.left) === Math.round(h.right - w.right)
  })
  expect(centred).toBe(true)

  await page.evaluate(() => {
    ;(document.getElementById('col') as HTMLElement).style.width = '366px'
  })
  await page.waitForTimeout(300)
  const narrow = await measure(page)
  expect(narrow.scale).toBeLessThan(1)
  expect(narrow.boxWidth).toBe(COLUMN)
})
