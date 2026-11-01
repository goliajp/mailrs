// Phone numbers a message asks the reader to call, and which of them the
// same sender has not written before in this conversation.
//
// A number proves nothing about who answers it, so this only points it
// out; it never judges the message. Pure, so it unit-tests as .test.ts.

type Sourced = { html_body: null | string; sender: string; text_body: null | string }

// a leading `+`, or digit groups joined by spaces, dots, dashes or
// brackets; the digit count decides below
const CANDIDATE = /(?:\+|\(\d)?\d[\d\s.()\-‐－]{7,22}\d/g
const MIN_DIGITS = 10
const MAX_DIGITS = 15
const MAX_SHOWN = 3

/**
 * Numbers in `message` that no earlier message from the same address in
 * `earlier` carried: a number a correspondent always signs with is not
 * worth pointing out, a new one asking to be called is.
 */
export function newCallbackNumbers(message: Sourced, earlier: readonly Sourced[]): string[] {
  const own = message.sender.toLowerCase()
  const before = new Set(
    earlier
      .filter((e) => e.sender.toLowerCase() === own)
      .flatMap((e) => phoneNumbers(plainText(e)).map(digits))
  )
  return phoneNumbers(plainText(message))
    .filter((n) => !before.has(digits(n)))
    .slice(0, MAX_SHOWN)
}

/** Numbers in `text` that read as phone numbers, as written, once each. */
export function phoneNumbers(text: string): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const match of text.matchAll(CANDIDATE)) {
    const written = match[0].trim()
    const d = digits(written)
    if (d.length < MIN_DIGITS || d.length > MAX_DIGITS) continue
    // an unbroken run is an order or account number, not a number to dial
    const grouped = written.startsWith('+') || /[\s.()\-‐－]/.test(written)
    // dates and times written with dots or dashes: 2026-11-01 12.30
    const dateLike = /^\d{4}[-.]\d{1,2}[-.]\d{1,2}/.test(written)
    if (!grouped || dateLike || seen.has(d)) continue
    seen.add(d)
    out.push(written)
  }
  return out
}

function digits(s: string): string {
  return s.replace(/\D/g, '')
}

function plainText(m: Sourced): string {
  if (m.text_body) return m.text_body
  if (!m.html_body) return ''
  return new DOMParser().parseFromString(m.html_body, 'text/html').body.textContent ?? ''
}
