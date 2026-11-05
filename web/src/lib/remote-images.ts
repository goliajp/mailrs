/**
 * Remote images in mail are never loaded from where a message points: the
 * server fetched each one when the mail arrived, and the page asks for
 * that copy by the URL's hash.
 */

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

/** What the reader is told about the images being fetched. */
export type RetryState = 'loading' | 'missing' | 'none'

/**
 * Ask again, on the `RETRY_MS` schedule, for the images that failed to
 * load once the server has been asked to fetch them. Only an image still
 * missing gets a new address: one that loaded keeps its own, and with it
 * the browser's copy. `onChange` hears 'loading' while some are missing,
 * 'none' once they have all come, and 'missing' when an image still fails
 * after the last attempt.
 */
export function retryMissing(onChange: (state: RetryState) => void) {
  const missing = new Set<HTMLImageElement>()
  let attempt = 0
  let running = false
  let ended = false
  let timer: ReturnType<typeof setTimeout> | undefined

  const loaded = (img: HTMLImageElement) => {
    missing.delete(img)
    if (running && missing.size === 0) onChange('none')
  }
  const next = () => {
    timer = setTimeout(() => {
      attempt += 1
      for (const img of missing) reload(img, attempt)
      if (attempt === RETRY_MS.length) {
        ended = true
        return
      }
      next()
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
      if (ended && missing.size > 0) onChange('missing')
      else if (running && missing.size > 0) onChange('loading')
    },
    /** The server has the images queued: start asking again. */
    start() {
      if (running) return
      running = true
      if (missing.size > 0) onChange('loading')
      next()
    },
    stop() {
      clearTimeout(timer)
    },
  }
}

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('')
}
