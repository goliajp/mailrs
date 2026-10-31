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

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('')
}
