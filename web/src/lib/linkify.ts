// Split a plain-text string into text / url segments so a renderer can
// wrap bare http(s) URLs in clickable anchors. HTML email bodies keep
// their own <a> tags (HtmlFrame) and markdown-ish bodies autolink via
// remark-gfm; this covers the remaining path — a text/plain body shown
// in a <pre>, where a bare URL would otherwise be dead text.
//
// Pure (no JSX) so it unit-tests as .test.ts.

export type Segment = { type: 'text' | 'url'; value: string }

// http(s) only — never javascript:/data: — so the anchor href is always
// safe to hand to the browser. Stops at whitespace and the quote/angle
// characters that can't appear unescaped in a URL.
const URL_RE = /https?:\/\/[^\s<>"']+/g

// trailing characters that a sentence appends after a URL but are not
// part of it: ASCII sentence punctuation + CJK punctuation + closing
// brackets. Trimmed back into the following text segment.
const TRAILING = /[.,;:!?)\]}、。，）】」』〉》]+$/

export function splitUrls(text: string): Segment[] {
  const out: Segment[] = []
  let last = 0
  for (const m of text.matchAll(URL_RE)) {
    const idx = m.index ?? 0
    let url = m[0]
    let tail = ''
    const t = url.match(TRAILING)
    if (t) {
      tail = t[0]
      url = url.slice(0, url.length - tail.length)
    }
    if (idx > last) out.push({ type: 'text', value: text.slice(last, idx) })
    out.push({ type: 'url', value: url })
    if (tail) out.push({ type: 'text', value: tail })
    last = idx + m[0].length
  }
  if (last < text.length) out.push({ type: 'text', value: text.slice(last) })
  return out
}
