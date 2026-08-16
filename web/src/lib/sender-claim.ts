/**
 * When a sender's display name claims one domain and the mail came from
 * another.
 *
 * The thread header shows a display name and rarely an address, which is
 * precisely the gap brand impersonation lives in: `Amazon.co.jp` reads
 * as Amazon whether it was sent by Amazon or by `mail07.jqjintaiyang.com`.
 *
 * **Measured before it was built**, on this deployment's 33,583 stored
 * messages: 4,825 display names (14.4%) contain a domain-shaped token,
 * and **83** of them — 0.247% of all mail — name a domain that did not
 * send it. Some are the same company's second domain (`GMO-Z.com` from
 * `gmo-runsystem.net`, `tuya.com` from `ismartlife.me`); most are
 * unmistakable, including `golia.jp | HR` sent from `halfwaylexus.cam`,
 * which is this deployment being impersonated to its own users.
 *
 * It **states** rather than accuses: "sent from X" is useful even when X
 * turns out to be legitimate, so a false positive costs the reader
 * nothing. That property is what makes it safe to show on a signal that
 * cannot be perfect.
 *
 * ## Why this exists beside the character check
 *
 * `mailrs_textguard` deliberately does **not** flag `U+FEFF`, `U+200C`,
 * `U+200D` or `U+00AD` — they have real typographic jobs and 59
 * production messages carry them legitimately. Three production phish
 * use exactly those to break up `Amazon` inside a display name, and so
 * pass the character check by construction. This catches them. The two
 * signals are complementary on purpose.
 *
 * Ported from `ios/Mailrs/Wire/SenderClaim.swift`, which carries the
 * original 1,500-message measurement and the record of the one candidate
 * signal that was measured and rejected (link text versus href fired on
 * 7% of ordinary mail — almost all of it click tracking).
 */

/**
 * Suffixes that make a token read as a domain.
 *
 * Deliberately a short common list rather than the public suffix list:
 * the point is to catch what a person would read *as* a domain, and
 * `amazon.co.jp` in a display name is that whether or not a full PSL
 * agrees about its boundaries.
 */
const SUFFIXES = new Set([
  'ai',
  'app',
  'cn',
  'co',
  'com',
  'dev',
  'io',
  'jp',
  'me',
  'net',
  'org',
  'shop',
])

/**
 * The first domain-shaped token in a display name.
 *
 * Splits on everything a DNS label cannot contain, so `【Amazon.co.jp】`
 * and `Amazon.co.jp 配信システム` both yield the token — and so does a
 * name padded with zero-width characters, since those are separators
 * here too.
 */
export function claimedDomain(displayName: string): null | string {
  const lowered = displayName.toLowerCase()
  for (const raw of lowered.split(/[^0-9a-z.-]+/)) {
    const candidate = raw.replace(/^[.-]+|[.-]+$/g, '')
    const labels = candidate.split('.')
    if (labels.length < 2) continue
    if (!SUFFIXES.has(labels[labels.length - 1])) continue
    if (labels.slice(0, -1).some((l) => l === '')) continue
    // A bare `co.jp` is a suffix, not a claim.
    if (labels.length === 2 && SUFFIXES.has(labels[0])) continue
    return candidate
  }
  return null
}

/**
 * The domain the mail actually came from, when the display name says a
 * different one. `null` when the name claims nothing, when the address
 * cannot be read, or when the two agree.
 *
 * Both arguments accept a full `Name <addr>` header, because that is
 * what the wire carries in `sender`; pass the same string twice when
 * that is all there is.
 */
export function contradictedDomain(sender: string, address: string): null | string {
  const sending = domainOf(address) ?? domainOf(sender)
  if (!sending) return null
  const claimed = claimedDomain(extractDisplayName(sender))
  if (!claimed) return null
  if (domainsAgree(claimed, sending)) return null
  return sending
}

/**
 * Related enough that saying so would be noise — the claim being a
 * subdomain of the sender, or the sender of the claim. Mail from
 * `email.amazon.co.jp` for a name saying `amazon.co.jp` is the ordinary
 * case and must stay silent.
 */
export function domainsAgree(claimed: string, sending: string): boolean {
  if (claimed === sending) return true
  if (sending.endsWith(`.${claimed}`)) return true
  if (claimed.endsWith(`.${sending}`)) return true
  return false
}

/** The domain of an address, or null when there isn't one to read. */
function domainOf(address: string): null | string {
  const email = extractEmail(address).toLowerCase()
  const at = email.lastIndexOf('@')
  if (at < 0) return null
  const domain = email.slice(at + 1)
  if (!domain.includes('.')) return null
  return domain
}

/** The display-name part of a `Name <addr>` header, unquoted. */
function extractDisplayName(sender: string): string {
  const angled = /<[^>]*>/.exec(sender)
  const name = angled ? sender.slice(0, angled.index) : sender
  return name.trim().replace(/^"|"$/g, '')
}

/** The address part of a `Name <addr>` header, or the string itself. */
function extractEmail(sender: string): string {
  const angled = /<([^>]*)>/.exec(sender)
  if (angled) return angled[1].trim()
  return sender.trim()
}
