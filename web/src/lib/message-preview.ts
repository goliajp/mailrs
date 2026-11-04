import type { Key } from '@/i18n'
import type { ThreadMessage } from '@/lib/types'

import DOMPurify from 'dompurify'

// strip invisible unicode: ZWJ, ZWNJ, ZW space, BOM, soft hyphen, directional marks, etc.
const INVISIBLE_RE =
  // eslint-disable-next-line no-misleading-character-class
  /[\u200B-\u200F\u2028-\u202F\u2060-\u2064\uFEFF\u00AD\u034F\u061C\u180E]/g

// box-drawing, table borders, repeated decorative lines
const NOISE_LINE = /^[\s│┼┬┴├┤┌┐└┘─━═╌╍╎╏║╔╗╚╝╠╣╦╩╬\-=_·•*#|+:>{}[\]~`]+$/

/** What a collapsed row says when its message has no readable text. */
export function previewPlaceholder(msg: ThreadMessage): Key {
  return placeholderLabel(Boolean(msg.html_body))
}

/**
 * A line of a message for its collapsed row: the summary when there is
 * one, else the words a reader would see in it, without the subject when
 * the body opens with it. Empty when nothing readable is left.
 */
export function previewText(msg: ThreadMessage, subject: string): string {
  const text = bubbleText(msg)
  if (!text) return ''
  const trimmed = stripSubjectFromPreview(text, subject)
  if (trimmed.length > 200) return smartTruncate(trimmed, 200)
  return trimmed
}

function bubbleText(msg: ThreadMessage): string {
  // prefer AI summary — always clean and readable
  if (msg.summary) return msg.summary
  // when html exists, extract the visible text from it. this avoids the
  // text/plain dump (markdown-table style `|cell |cell |`) that html-only
  // senders often produce; htmlToPreviewText reads the same words a human
  // would read in the rendered email.
  if (msg.html_body) {
    const fromHtml = htmlToPreviewText(msg.html_body)
    if (fromHtml.length > 0) return fromHtml
  }
  const raw = msg.new_content || msg.clean_text || msg.text_body || ''
  if (!raw) return ''
  // the dump check is for text parts only: html has been read as html
  // above, and its text folded to one line would trip it on any footer
  // of four links split by bars
  const text = cleanTextForBubble(raw)
  if (looksLikeHtmlDump(text)) return ''
  return text
}

function cleanTextForBubble(raw: string): string {
  const lines = raw.replace(INVISIBLE_RE, '').split('\n')

  // find signature delimiter and remove everything after it
  let sigIdx = lines.length
  for (let i = 0; i < lines.length; i++) {
    if (lines[i] === '-- ' || lines[i] === '--') {
      sigIdx = i
      break
    }
  }

  return lines
    .slice(0, sigIdx)
    .filter((line) => !NOISE_LINE.test(line))
    .filter((line) => !line.startsWith('>')) // remove quoted lines
    .map((line) => line.replace(/\s{2,}/g, ' ').trim())
    .filter(Boolean)
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

// strip the subject from the start of preview text. many transactional
// emails (Stripe receipts, GitHub notifications, etc.) have the subject
// repeated as the first heading inside the body, so the bubble would
// show the same line twice (once as the subject row, once as the
// preview). matches the leading words case-insensitively, with a small
// allowance for trailing punctuation / sender names.
function stripSubjectFromPreview(text: string, subject: string): string {
  if (!subject) return text
  const normSubject = subject.toLowerCase().trim()
  if (!normSubject || normSubject.length < 6) return text
  const normText = text.toLowerCase()
  // search the first 200 chars for the subject and start the preview
  // after it (skipping common separator characters)
  const window = normText.slice(0, Math.min(200, normText.length))
  const at = window.indexOf(normSubject)
  if (at === -1) return text
  let cut = at + normSubject.length
  while (cut < text.length && /[\s·•|–—\-:,]/.test(text[cut])) cut++
  const remainder = text.slice(cut).trim()
  return remainder.length >= 30 ? remainder : text
}

// dedicated DOMPurify instance so this preview path never runs the
// global hooks (e.g. anchor target=_blank) that the message body uses
const previewPurifier = DOMPurify()

// extract a clean reading-order preview from html.
// the simple "strip all tags, keep textContent" approach pulled in url
// soup: anchors whose visible text is the URL itself, footer-style
// '[1]: https://…' link lists, and tracking redirects that stretch
// hundreds of characters with no break point. all of that overflowed
// the bubble and added zero signal. we now strip those before
// collapsing to text.
function htmlToPreviewText(html: string): string {
  // sanitize with structure preserved (so we can walk anchors), drop
  // dangerous tags + the noisy ones (head/style/script don't appear in
  // textContent anyway, but iframe/svg/img title attributes can leak).
  const cleanHtml = previewPurifier.sanitize(html, {
    FORBID_ATTR: ['style'],
    FORBID_TAGS: ['script', 'style', 'svg', 'iframe', 'img'],
  })
  // DOMParser is browser-only; we run in vite/jsdom so this is fine
  const doc = new DOMParser().parseFromString(`<div>${cleanHtml}</div>`, 'text/html')
  const root = doc.body.firstElementChild ?? doc.body
  dropQuotedMail(root)
  // textContent runs adjacent blocks together ('me.On Mon'); end each
  // block with a space, which the fold below turns into one
  for (const el of Array.from(root.querySelectorAll(BLOCKS))) el.after(' ')

  // remove anchors whose visible text is just a URL — they're tracking
  // redirects ('https://c.gle/…' / 'https://email.stripe.com/…') that
  // bring no information into a tiny preview
  for (const a of Array.from(root.querySelectorAll('a'))) {
    const text = (a.textContent || '').trim()
    const href = a.getAttribute('href') || ''
    if (text === '' || /^https?:\/\//i.test(text) || text === href) {
      a.remove()
    }
  }

  const text = (root.textContent || '')
    // footer-style link lists: '[1]: https://…' / '[12]:https://…'
    .replace(/\[\d+\]:\s*https?:\/\/\S+/gi, '')
    // standalone bracketed URL refs '[ https://… ]' some clients emit
    .replace(/\[\s*https?:\/\/\S+\s*\]/gi, '')
    // any remaining long URL — collapse to placeholder so it can never
    // overflow the bubble or eat the entire 3-line clamp
    .replace(/https?:\/\/\S+/gi, '[link]')
    .replace(/&nbsp;/gi, ' ')
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    // clean up footnote leftovers: <sup>1</sup> and <sub>x</sub> get
    // dropped to bare characters that look like '^' / orphan digits in
    // the middle of CJK text. strip the common ones.
    .replace(/[\u00B9\u00B2\u00B3\u2070-\u2079\u2080-\u2089]/g, '')
    .replace(/(?<=[\p{L}\p{N}])\^(?=[。\s,])/gu, '')
    .replace(/\s+/g, ' ')
    .trim()
  return text
}

const BLOCKS =
  'address, article, br, dd, div, dl, dt, footer, h1, h2, h3, h4, h5, h6, header, hr, li, ' +
  'ol, p, pre, section, table, td, th, tr, ul'

// the earlier mail a reply quotes, as the common clients mark it: gmail,
// apple mail and thunderbird nest it in a blockquote or a marked block
const QUOTED = 'blockquote, .gmail_quote, .gmail_extra, .yahoo_quoted, .moz-cite-prefix'
// outlook does not nest it: everything after its reply header is the
// earlier mail
const OUTLOOK_REPLY = '#appendonsend, #divRplyFwdMsg'

function dropQuotedMail(root: Element) {
  for (const el of Array.from(root.querySelectorAll(QUOTED))) el.remove()
  const reply = root.querySelector(OUTLOOK_REPLY)
  if (!reply) return
  for (let el: Element | null = reply; el && el !== root; el = el.parentElement) {
    while (el.nextSibling) el.nextSibling.remove()
  }
  reply.remove()
}

// last-resort detection: when only a text/plain part exists and it looks
// like html-to-text noise (markdown-table dump, jammed brackets), drop
// it rather than render garbage. lower threshold + extra heuristic for
// any single line carrying 4+ pipes (clear table-row signature).
function looksLikeHtmlDump(text: string): boolean {
  if (!text || text.length < 40) return false
  const noise = (text.match(/[|{}[\]<>]/g) || []).length
  if (noise / text.length > 0.05) return true
  const lines = text.split('\n')
  if (lines.length >= 5) {
    const longLines = lines.filter((l) => l.length > 200).length
    if (longLines / lines.length > 0.3) return true
  }
  return lines.some((l) => (l.match(/\|/g) || []).length >= 4)
}

function placeholderLabel(hasHtml: boolean): Key {
  if (hasHtml) return 'read.richPreview'
  return 'read.noPreview'
}

// truncate at nearest sentence/paragraph boundary instead of hard character cut
function smartTruncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text
  const sub = text.slice(0, maxLen)
  // try to break at paragraph
  const lastNewline = sub.lastIndexOf('\n')
  if (lastNewline > maxLen * 0.5) return sub.slice(0, lastNewline).trimEnd()
  // try to break at sentence (。.!?！？)
  const sentenceEnd = sub.search(/[.。!！?？]\s*[^\s]*$/)
  if (sentenceEnd > maxLen * 0.4) return sub.slice(0, sentenceEnd + 1).trimEnd()
  // fall back to word boundary
  const lastSpace = sub.lastIndexOf(' ')
  if (lastSpace > maxLen * 0.5) return sub.slice(0, lastSpace).trimEnd() + '…'
  return sub.trimEnd() + '…'
}
