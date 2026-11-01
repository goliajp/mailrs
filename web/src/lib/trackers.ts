/**
 * Trackers in received mail, by the rules the server uses: the same
 * `trackers.json`, a byte-identical copy of
 * crates/intake/src/image_prefetch/trackers.json (a Rust test compares them).
 * The server never fetches these images; this hides them and counts them.
 */
import rules from './trackers.json'

const TRACKER = 'data-tracker'

/** Take a tracker out of the page: no address left to load, its host kept to name it. */
export function disarmTracker(img: Element): void {
  img.setAttribute(TRACKER, hostOf(img.getAttribute('src') ?? ''))
  img.removeAttribute('src')
  img.removeAttribute('srcset')
}

/** Whether this `<img>` only tells its sender the mail was read. */
export function isTrackingImage(img: Element): boolean {
  const src = (img.getAttribute('src') ?? '').trim()
  if (!/^https?:\/\//i.test(src)) return false
  return trackingUrl(src) || invisible(img)
}

/** `url` without the query parameters that only identify who clicked. */
export function stripTrackingParams(url: string): string {
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return url
  }
  const doomed = [...parsed.searchParams.keys()].filter((key) => {
    const k = key.toLowerCase()
    return rules.params.includes(k) || rules.param_prefixes.some((p) => k.startsWith(p))
  })
  if (doomed.length === 0) return url
  for (const key of doomed) parsed.searchParams.delete(key)
  return parsed.toString()
}

/** How many trackers were taken out of sanitised `html`. */
export function trackerCount(html: string): number {
  return (html.match(/data-tracker="/g) ?? []).length
}

/** The hosts of the trackers taken out of sanitised `html`, once each. */
export function trackersIn(html: string): string[] {
  const hosts = new Set<string>()
  for (const m of html.matchAll(/data-tracker="([^"]*)"/g)) hosts.add(m[1])
  return [...hosts]
}

/** The host an image or link points at, lowercased; '' when there is none. */
function hostOf(url: string): string {
  try {
    return new URL(url).hostname.toLowerCase()
  } catch {
    return ''
  }
}

/** Drawn at a pixel or less on each side it states, or hidden outright. */
function invisible(img: Element): boolean {
  const style = (img.getAttribute('style') ?? '').toLowerCase().replace(/\s+/g, '')
  if (style.includes('display:none') || style.includes('visibility:hidden')) return true
  const fromStyle = (side: string) => {
    const m = new RegExp(`(?:^|;)${side}:(\\d+)(px)?(?:;|$)`).exec(style)
    if (!m) return null
    return Number(m[1])
  }
  const sides = [
    px(img.getAttribute('width')) ?? fromStyle('width'),
    px(img.getAttribute('height')) ?? fromStyle('height'),
  ].filter((s): s is number => s !== null)
  return sides.length > 0 && sides.every((s) => s <= 1)
}

/** `1`, `1px`, `0`; null for anything relative or absent. */
function px(value: null | string): null | number {
  if (value === null) return null
  const m = /^\s*(\d+)\s*(px)?\s*$/i.exec(value)
  if (!m) return null
  return Number(m[1])
}

function trackingUrl(src: string): boolean {
  let url: URL
  try {
    url = new URL(src)
  } catch {
    return false
  }
  const host = url.hostname.toLowerCase()
  const path = url.pathname.toLowerCase()
  return (
    rules.hosts.some((h) => host === h || host.endsWith(`.${h}`)) ||
    rules.paths.some((p) => path.includes(p))
  )
}
