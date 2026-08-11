export type UnsubscribeHeader = null | {
  http?: readonly string[]
  mailto?: readonly string[]
  one_click?: boolean
}

/**
 * What to offer a reader who wants off a list, given what the message
 * said.
 *
 * Three answers, not one, because they cost the reader different
 * things and only one is free:
 *
 * - **one-click** — the server leaves the list on the reader's behalf
 *   (RFC 8058). Nothing of theirs reaches the sender.
 * - **a page** — their IP and user agent reach the sender the moment
 *   it loads, so this is offered as a link and never performed for
 *   them.
 * - **an address** — handed to the composer with the subject and body
 *   the sender asked for, because that is usually what it keys on.
 *
 * A pure function so the rule lives in one place instead of being
 * inferred from a chain of conditionals in a component — and so it
 * matches iOS's `UnsubscribeOffer`, which is the same rule.
 */
export type UnsubscribeOffer =
  | { kind: 'mailto'; url: string }
  | { kind: 'none' }
  | { kind: 'one-click' }
  | { kind: 'page'; url: string }

export function unsubscribeOffer(header: undefined | UnsubscribeHeader): UnsubscribeOffer {
  if (!header) return { kind: 'none' }
  if (header.one_click) return { kind: 'one-click' }
  // A page before an address: it is one click against composing and
  // sending a message, and senders offering both treat them the same.
  const page = (header.http ?? []).find((u) => u.startsWith('http'))
  if (page) return { kind: 'page', url: page }
  const address = (header.mailto ?? []).find((u) => u.startsWith('mailto:'))
  if (address) return { kind: 'mailto', url: address }
  return { kind: 'none' }
}
