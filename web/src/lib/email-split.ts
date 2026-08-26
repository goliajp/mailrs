export type EmailParts = {
  body: string
  quoted: null | string
}

type SplitResult = { isHtml: boolean; parts: EmailParts }

// detect "On ... wrote:" attribution line
const ATTRIBUTION_RE = /^.{0,200}\bwrote:\s*$/

// detect Outlook-style original message separator
const OUTLOOK_SEP_RE = /^-{4,}\s*Original Message\s*-{4,}$/i

// Module-level LRU. `splitHtmlEmail` constructs a fresh DOMParser document
// and walks it for quoted-block selectors — for newsletter-sized
// bodies that's 50-200 ms per call. MessageBubble's useMemo is component-
// scoped, so unmounting (every thread switch) threw away the memoization.
// Keying on the raw body identity here makes thread-switch-back free.
// The cache map is declared here next to the type it stores; the get/put
// helpers it backs are defined below the public `splitEmail` to satisfy
// module-export ordering rules.
const MAX_CACHE_ENTRIES = 100
const splitCache = new Map<string, SplitResult>()

export function splitEmail(textBody: null | string, htmlBody: null | string): SplitResult {
  // Both inputs, because the split depends on both. It keyed on the
  // html alone whenever there was any, which was harmless only while
  // the html decided the result by itself. A blank-painting html body
  // falls back to the text now, so two messages sharing a
  // stylesheet-only html and differing in their text would otherwise
  // have the first one through decide what the second displays.
  //
  // The length prefix is what keeps the two halves from running
  // together: without it, ("ab", "c") and ("a", "bc") are one key.
  const t = textBody ?? ''
  const h = htmlBody ?? ''
  const cacheKey = `${t.length}:${t}\u0000${h}`
  const cached = cacheGet(cacheKey)
  if (cached) return cached
  // An html body that paints nothing is not content, so the text is what
  // the message actually says. The reading pane has made this choice
  // since A1; the bubble reads `isHtml` from here, so making it here is
  // what gives both the same answer from one rule.
  //
  // Only when there is text to fall back to: a blank-painting html with
  // no text at all is still all the message has, and rendering an empty
  // text part in its place shows less, not more.
  const useHtml = !!htmlBody && !(t.trim() && htmlBodyPaintsNothing(htmlBody))
  let result: SplitResult
  try {
    result = useHtml
      ? unsplitIfEmpty(
          { isHtml: true, parts: splitHtmlEmail(htmlBody as string) },
          htmlBody as string
        )
      : unsplitIfEmpty({ isHtml: false, parts: splitTextEmail(t) }, t)
  } catch {
    // fallback: return as-is
    result = {
      isHtml: useHtml,
      parts: {
        body: useHtml ? (htmlBody ?? '') : (textBody ?? htmlBody ?? ''),
        quoted: null,
      },
    }
  }
  cachePut(cacheKey, result)
  return result
}

export function splitHtmlEmail(html: string): EmailParts {
  const parser = new DOMParser()
  const doc = parser.parseFromString(html, 'text/html')

  let quoted: null | string = null

  // **The signature stays in the body.** It used to be pulled out into
  // a field of its own — and nothing anywhere rendered that field, so
  // pulling it out meant deleting it.
  //
  // Which would be a small loss if the marker were reliable, and it is
  // not: Outlook Web writes `id="Signature"` around whatever sits in
  // the composing area, so a message sent from it can have its entire
  // text inside that div. One arrived on 2026-08-26 whose whole body —
  // "I trust you are doing well! … Best Regards, Maria Carter" — was in
  // there, and the reader saw the greeting above it and nothing else.
  // The preview beside it, which flattens the raw html, showed the
  // sentence the pane was hiding.

  // extract quoted text by client-specific selectors
  // Gmail
  const gmailQuote = doc.body.querySelector('.gmail_quote')
  if (gmailQuote) {
    quoted = gmailQuote.innerHTML.trim()
    gmailQuote.remove()
  }

  // Outlook: #divRplyFwdMsg + all following siblings
  if (!quoted) {
    const outlookDiv =
      doc.body.querySelector('#divRplyFwdMsg') ?? doc.body.querySelector('#appendonsend')
    if (outlookDiv) {
      const parts: string[] = []
      let node: Element | null = outlookDiv
      while (node) {
        parts.push(node.outerHTML)
        const sibling: Element | null = node.nextElementSibling
        node.remove()
        node = sibling
      }
      quoted = parts.join('')
    }
  }

  // Yahoo
  if (!quoted) {
    const yahooQuote = doc.body.querySelector('.yahoo_quoted')
    if (yahooQuote) {
      quoted = yahooQuote.innerHTML.trim()
      yahooQuote.remove()
    }
  }

  // Mozilla: .moz-cite-prefix + following blockquote[type="cite"]
  if (!quoted) {
    const mozPrefix = doc.body.querySelector('.moz-cite-prefix')
    if (mozPrefix) {
      const parts: string[] = [mozPrefix.outerHTML]
      let next: Element | null = mozPrefix.nextElementSibling
      mozPrefix.remove()
      while (next && next.tagName === 'BLOCKQUOTE' && next.getAttribute('type') === 'cite') {
        parts.push(next.outerHTML)
        const following = next.nextElementSibling
        next.remove()
        next = following
      }
      quoted = parts.join('')
    }
  }

  // Apple Mail / generic: top-level blockquote[type="cite"]
  if (!quoted) {
    const citeBlock = doc.body.querySelector('blockquote[type="cite"]')
    if (citeBlock) {
      quoted = citeBlock.outerHTML
      citeBlock.remove()
    }
  }

  // fallback: trailing <blockquote> (only if it's the last significant element)
  if (!quoted) {
    const children = Array.from(doc.body.children)
    if (children.length > 0) {
      const last = children[children.length - 1]
      // check if last element is a blockquote, or contains only a blockquote as last child
      if (last.tagName === 'BLOCKQUOTE') {
        quoted = last.innerHTML.trim()
        last.remove()
      } else {
        const innerChildren = Array.from(last.children)
        if (innerChildren.length > 0) {
          const innerLast = innerChildren[innerChildren.length - 1]
          if (innerLast.tagName === 'BLOCKQUOTE') {
            quoted = innerLast.innerHTML.trim()
            innerLast.remove()
          }
        }
      }
    }
  }

  const body = doc.body.innerHTML.trim()

  return { body, quoted: quoted || null }
}

export function splitTextEmail(text: string): EmailParts {
  if (!text) return { body: '', quoted: null }

  const lines = text.split('\n')

  // scan from bottom to find the start of quoted text
  let quotedStart = -1

  // look for attribution line ("On ... wrote:") followed by quoted lines
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i]
    if (ATTRIBUTION_RE.test(line)) {
      // verify at least one `>` line follows
      const hasQuotedBelow = lines.slice(i + 1).some((l) => l.startsWith('>'))
      if (hasQuotedBelow) {
        quotedStart = i
        break
      }
    }
    if (OUTLOOK_SEP_RE.test(line)) {
      quotedStart = i
      break
    }
  }

  // if no attribution found, look for a trailing block of `>` lines
  if (quotedStart === -1) {
    let lastQuoted = -1
    for (let i = lines.length - 1; i >= 0; i--) {
      if (lines[i].startsWith('>')) {
        if (lastQuoted === -1) lastQuoted = i
        quotedStart = i
      } else if (lines[i].trim() === '' && quotedStart !== -1) {
        // allow blank lines within quoted block
        continue
      } else if (quotedStart !== -1) {
        break
      }
    }
    if (quotedStart !== -1) {
      // trim leading blank lines before the first `>` line
      while (
        quotedStart > 0 &&
        lines[quotedStart].trim() === '' &&
        !lines[quotedStart].startsWith('>')
      ) {
        quotedStart++
      }
    }
  }

  // extract quoted section
  let quoted: null | string = null
  let remaining = lines
  if (quotedStart !== -1) {
    quoted = lines.slice(quotedStart).join('\n').trimEnd()
    remaining = lines.slice(0, quotedStart)
  }

  // The `-- ` signature stays in the body too, for the same reason:
  // nothing rendered it, so cutting it here removed the sender's name
  // from what the reader could see.
  const body = remaining.join('\n').trimEnd()

  return { body, quoted }
}

function cacheGet(key: string): SplitResult | undefined {
  const hit = splitCache.get(key)
  if (hit === undefined) return undefined
  // refresh recency
  splitCache.delete(key)
  splitCache.set(key, hit)
  return hit
}

function cachePut(key: string, value: SplitResult): void {
  splitCache.set(key, value)
  while (splitCache.size > MAX_CACHE_ENTRIES) {
    const oldest = splitCache.keys().next().value
    if (oldest === undefined) break
    splitCache.delete(oldest)
  }
}

/**
 * Whether a split body has anything left to look at.
 *
 * Text content alone is not the test: a body of one inline image has no text
 * and is not empty. `<br>` and `&nbsp;` are, which is exactly what a Gmail
 * forward leaves behind.
 */
function isVisuallyEmpty(body: string, isHtml: boolean): boolean {
  if (!isHtml) return body.trim() === ''
  return htmlBodyPaintsNothing(body)
}

// Elements that occupy space on their own, with no text inside them.
// `img` is handled separately \u2014 see `imageIsSomethingToLookAt`.
const PAINTS_ON_ITS_OWN = 'video, audio, table, iframe, svg, canvas, hr'

const HIDDEN_STYLE_RE =
  /(?:^|;)\s*(?:display\s*:\s*none|visibility\s*:\s*hidden|opacity\s*:\s*0(?:\.0+)?)\s*(?:;|$)/i

/**
 * Whether an HTML body would paint anything a reader can see.
 *
 * The reader pane chose the HTML branch on `html_body` being non-empty,
 * which is not the same question. A mailing sent through Odoo/SES on
 * 2026-08-14 arrived with its body missing from both MIME parts: 2.4 kB
 * of `<style>` in the head, a `display:none` preheader, and a tracking
 * gif. Every layer handled it correctly and the reader saw a white box \u2014
 * worse than the `(no text content)` line, because it looks like a
 * failure to load rather than an empty message.
 *
 * Three things are deliberately not content: a stylesheet, a subtree the
 * message itself hides, and a *remote* image that declares neither a size
 * nor alt text in a body that has no text at all. The last is the only
 * judgement call, and it is narrow: an inline `cid:` or `data:` image is
 * an attachment the sender chose to embed and always counts, and a remote
 * image that says how big it is or what it shows counts too. What is left
 * \u2014 a bare URL, no size, no alt, no text anywhere near it \u2014 is a beacon.
 */
export function htmlBodyPaintsNothing(html: string): boolean {
  try {
    const doc = new DOMParser().parseFromString(html, 'text/html')
    for (const el of Array.from(doc.body.querySelectorAll('style, script, template'))) {
      el.remove()
    }
    // Removing a hidden ancestor takes its descendants with it, so this
    // walks a shrinking tree \u2014 re-query rather than iterate a stale list.
    for (const el of Array.from(doc.body.querySelectorAll('[style]'))) {
      if (el.isConnected && HIDDEN_STYLE_RE.test(el.getAttribute('style') ?? '')) el.remove()
    }
    if (doc.body.textContent?.replace(/\u00a0/g, ' ').trim() !== '') return false
    if (doc.body.querySelector(PAINTS_ON_ITS_OWN)) return false
    return !Array.from(doc.body.querySelectorAll('img')).some(imageIsSomethingToLookAt)
  } catch {
    return html.trim() === ''
  }
}

function imageIsSomethingToLookAt(img: Element): boolean {
  if ((img.getAttribute('alt') ?? '').trim() !== '') return true
  // An embedded part, not a fetch: nothing is learned about the reader by
  // showing it, and the sender put it there on purpose.
  if (!/^\s*https?:/i.test(img.getAttribute('src') ?? '')) return true
  const style = img.getAttribute('style') ?? ''
  const sized = [
    img.getAttribute('width'),
    img.getAttribute('height'),
    /(?:^|;)\s*width\s*:\s*([^;]+)/i.exec(style)?.[1] ?? null,
    /(?:^|;)\s*height\s*:\s*([^;]+)/i.exec(style)?.[1] ?? null,
  ]
  return sized.some(isBiggerThanAPixel)
}

function isBiggerThanAPixel(declared: null | string): boolean {
  if (declared === null) return false
  const n = parseFloat(declared.trim())
  if (!Number.isFinite(n)) return false
  return declared.trim().endsWith('%') ? n > 0 : n > 1
}

/**
 * Splitting must never leave nothing to read.
 *
 * A **forward** puts the entire message inside the quote block — Gmail emits
 * `<div dir="ltr"><br><br><div class="gmail_quote gmail_quote_container">…`
 * with everything in that inner div. Extracting it left a body of two
 * `<br>`s, and since MessageBubble renders only `parts.body` and never
 * `parts.quoted`, the content was not collapsed but discarded: a forwarded
 * mail showed a blank band the height of two line breaks (2026-07-30).
 *
 * Decided by what is left rather than by looking for "Forwarded message" —
 * that string is localized, and the message that surfaced this carried a mix
 * of English and Chinese in one header block.
 */
function unsplitIfEmpty(result: SplitResult, original: string): SplitResult {
  const extracted = result.parts.quoted !== null
  if (!extracted) return result
  if (!isVisuallyEmpty(result.parts.body, result.isHtml)) return result
  return {
    isHtml: result.isHtml,
    parts: { body: original, quoted: null },
  }
}
