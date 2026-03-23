export type EmailParts = {
  body: string
  signature: string | null
  quoted: string | null
}

// detect "On ... wrote:" attribution line
const ATTRIBUTION_RE = /^.{0,200}\bwrote:\s*$/

// detect Outlook-style original message separator
const OUTLOOK_SEP_RE = /^-{4,}\s*Original Message\s*-{4,}$/i

export function splitTextEmail(text: string): EmailParts {
  if (!text) return { body: '', signature: null, quoted: null }

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
  let quoted: string | null = null
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

  let signature: string | null = null
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

  return { body, signature, quoted }
}

export function splitHtmlEmail(html: string): EmailParts {
  const parser = new DOMParser()
  const doc = parser.parseFromString(html, 'text/html')

  let signature: string | null = null
  let quoted: string | null = null

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
    signature: signature || null,
    quoted: quoted || null,
  }
}

export function splitEmail(
  textBody: string | null,
  htmlBody: string | null,
): { parts: EmailParts; isHtml: boolean } {
  try {
    if (htmlBody) {
      return { parts: splitHtmlEmail(htmlBody), isHtml: true }
    }
    return { parts: splitTextEmail(textBody ?? ''), isHtml: false }
  } catch {
    // fallback: return as-is
    return {
      parts: {
        body: htmlBody ?? textBody ?? '',
        signature: null,
        quoted: null,
      },
      isHtml: !!htmlBody,
    }
  }
}
