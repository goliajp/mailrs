/**
 * Remote images in mail are never loaded from where a message points: the
 * server fetched each one when the mail arrived, and the page asks for
 * that copy by the URL's hash.
 */

import { http } from '@/lib/http'

const REMOTE_SRC = 'data-remote-src'

/** Take an image's remote address off it before it reaches the page. */
export function markRemoteImage(node: Element): void {
  if (node.tagName !== 'IMG') return
  const src = (node.getAttribute('src') ?? '').trim()
  if (!/^https?:\/\//i.test(src)) return
  node.setAttribute(REMOTE_SRC, src)
  node.removeAttribute('src')
}

/** Ask for an image again, past anything the browser kept of the miss. */
export function reload(img: HTMLImageElement, attempt: number): void {
  const base = img.src.split('?')[0]
  img.src = `${base}?attempt=${attempt}`
}

/** Point each marked image at the copy the server prefetched. */
export async function showPrefetched(root: ParentNode): Promise<void> {
  for (const img of root.querySelectorAll<HTMLImageElement>(`img[${REMOTE_SRC}]`)) {
    const url = img.getAttribute(REMOTE_SRC) ?? ''
    img.src = `/api/mail/image/${await sha256Hex(url)}`
  }
}

/**
 * Call `onMissing` once the marked images that could not be loaded are
 * known: every failure in a burst, together.
 */
export function watchMissing(
  root: ParentNode,
  onMissing: (imgs: HTMLImageElement[]) => void
): void {
  let failed: HTMLImageElement[] = []
  let timer: ReturnType<typeof setTimeout> | undefined
  for (const img of root.querySelectorAll<HTMLImageElement>(`img[${REMOTE_SRC}]`)) {
    img.addEventListener('error', () => {
      failed.push(img)
      clearTimeout(timer)
      timer = setTimeout(() => {
        const batch = failed
        failed = []
        onMissing(batch)
      }, 500)
    })
  }
}

/**
 * When to ask again for images the server was asked to fetch, as gaps
 * between attempts. Short at first: the server fetches what a reader asks
 * for ahead of everything else, and that usually lands within seconds.
 * Then longer, to about two minutes in all, which is what fifty images
 * from one sender take at the fetcher's thirty a minute per domain.
 */
export const RETRY_MS = [2_000, 4_000, 8_000, 15_000, 30_000, 30_000, 30_000]

/** What the server says of an image the page could not load. */
export type Probed = 'refused' | 'there' | 'waiting'

/** What the reader is told about the images being fetched. */
export type RetryState = 'loading' | 'missing' | 'none'

/**
 * Ask again, on the `RETRY_MS` schedule, for the images that failed to
 * load once the server has been asked to fetch them. Each attempt first
 * asks the server whether the image is there yet: one that is gets a new
 * address, one the server says it was refused (410) is waited for no
 * more, and the rest wait for the next attempt. An image that loaded keeps
 * its own address, and with it the browser's copy. `onChange` hears
 * 'loading' while some are awaited, 'none' once they have all come, and
 * 'missing' as soon as none is awaited and some will not come, or the
 * schedule ran out with some still away.
 */
export function retryMissing(
  onChange: (state: RetryState) => void,
  probe: (img: HTMLImageElement) => Promise<Probed> = probeImage
) {
  const missing = new Set<HTMLImageElement>()
  let refused = 0
  let attempt = 0
  let running = false
  let ended = false
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined

  const settle = () => {
    if (!running) return
    if (missing.size > 0 && !ended) {
      onChange('loading')
      return
    }
    if (missing.size > 0 || refused > 0) onChange('missing')
    else onChange('none')
  }
  const loaded = (img: HTMLImageElement) => {
    missing.delete(img)
    settle()
  }
  const next = () => {
    timer = setTimeout(() => {
      attempt += 1
      const tried = attempt
      void Promise.all(
        [...missing].map(async (img) => {
          const answer = await probe(img)
          if (!missing.has(img)) return
          if (answer === 'there') reload(img, tried)
          if (answer === 'refused') {
            missing.delete(img)
            refused += 1
          }
        })
      ).then(() => {
        if (stopped) return
        if (tried === RETRY_MS.length) ended = true
        settle()
        if (!ended && missing.size > 0) next()
      })
    }, RETRY_MS[attempt])
  }

  return {
    /** Images that failed to load, as `watchMissing` reports them. */
    add(imgs: HTMLImageElement[]) {
      for (const img of imgs) {
        if (missing.has(img)) continue
        missing.add(img)
        img.addEventListener('load', () => loaded(img), { once: true })
      }
      settle()
    },
    /** The server has the images queued: start asking again. */
    start() {
      if (running) return
      running = true
      settle()
      next()
    },
    stop() {
      stopped = true
      clearTimeout(timer)
    },
  }
}

/** Ask the server, without the body, whether an image has come. */
async function probeImage(img: HTMLImageElement): Promise<Probed> {
  const base = img.src.split('?')[0]
  try {
    const res = await http(base, { cache: 'no-store', method: 'HEAD' })
    if (res.ok) return 'there'
    if (res.status === 410) return 'refused'
    return 'waiting'
  } catch {
    return 'waiting'
  }
}

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('')
}
