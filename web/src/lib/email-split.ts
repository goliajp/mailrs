export type EmailParts = {
  body: string
  quoted: null | string
  signature: null | string
}

type SplitResult = { isHtml: boolean; parts: EmailParts }

// detect "On ... wrote:" attribution line
const ATTRIBUTION_RE = /^.{0,200}\bwrote:\s*$/

// detect Outlook-style original message separator
const OUTLOOK_SEP_RE = /^-{4,}\s*Original Message\s*-{4,}$/i

// Module-level LRU. `splitHtmlEmail` constructs a fresh DOMParser document
// and walks it for signature / quoted-block selectors — for newsletter-sized
// bodies that's 50-200 ms per call. MessageBubble's useMemo is component-
// scoped, so unmounting (every thread switch) threw away the memoization.
// Keying on the raw body identity here makes thread-switch-back free.
// The cache map is declared here next to the type it stores; the get/put
// helpers it backs are defined below the public `splitEmail` to satisfy
// module-export ordering rules.
const MAX_CACHE_ENTRIES = 100
const splitCache = new Map<string, SplitResult>()

export function splitEmail(textBody: null | string, htmlBody: null | string): SplitResult {
  const cacheKey = htmlBody ? `h:${htmlBody}` : `t:${textBody ?? ''}`
  const cached = cacheGet(cacheKey)
  if (cached) return cached
  let result: SplitResult
  try {
    result = htmlBody
      ? unsplitIfEmpty({ isHtml: true, parts: splitHtmlEmail(htmlBody) }, htmlBody)
      : unsplitIfEmpty({ isHtml: false, parts: splitTextEmail(textBody ?? '') }, textBody ?? '')
  } catch {
    // fallback: return as-is
    result = {
      isHtml: !!htmlBody,
      parts: {
        body: htmlBody ?? textBody ?? '',
        quoted: null,
        signature: null,
      },
    }
  }
  cachePut(cacheKey, result)
  return result
}

export function splitHtmlEmail(html: string): EmailParts {
  const parser = new DOMParser()
  const doc = parser.parseFromString(html, 'text/html')

  let signature: null | string = null
  let quoted: null | string = null

  // extract signature
  const sigSelectors = ['.gmail_signature', '#Signature', '#signature']
  for (const sel of sigSelectors) {
    const el = doc.body.querySelector(sel)
    if (el) {
      signature = el.innerHTML.trim()
      el.remove()
      break
    }
  }

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

  return {
    body,
    quoted: quoted || null,
    signature: signature || null,
  }
}

export function splitTextEmail(text: string): EmailParts {
  if (!text) return { body: '', quoted: null, signature: null }

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

  // find signature in remaining (non-quoted) text
  let sigStart = -1
  for (let i = remaining.length - 1; i >= 0; i--) {
    if (remaining[i] === '-- ' || remaining[i] === '--') {
      sigStart = i
      break
    }
  }

  let signature: null | string = null
  let bodyLines = remaining
  if (sigStart !== -1) {
    const sigContent = remaining
      .slice(sigStart + 1)
      .join('\n')
      .trimEnd()
    signature = sigContent || null
    bodyLines = remaining.slice(0, sigStart)
  }

  const body = bodyLines.join('\n').trimEnd()

  return { body, quoted, signature }
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
  try {
    const doc = new DOMParser().parseFromString(body, 'text/html')
    if (doc.body.querySelector('img, video, audio, table, iframe')) return false
    return doc.body.textContent?.replace(/\u00a0/g, ' ').trim() === ''
  } catch {
    return body.trim() === ''
  }
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
  const extracted = result.parts.quoted !== null || result.parts.signature !== null
  if (!extracted) return result
  if (!isVisuallyEmpty(result.parts.body, result.isHtml)) return result
  return {
    isHtml: result.isHtml,
    parts: { body: original, quoted: null, signature: null },
  }
}
