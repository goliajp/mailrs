/**
 * Whether a message may present itself as coming from you.
 *
 * The From header is written by whoever sent the message. A phish that puts
 * your own address in it gets, from a UI that believes headers, your name,
 * your avatar and the styling reserved for your own mail — while the badge
 * that would have given it away is suppressed precisely because the message
 * claimed to be yours.
 *
 * That happened on 2026-08-01: `Netflix <takagi@golia.jp>` arrived from a
 * Google Cloud host that announced itself as `mail.golia.ai`, failed SPF,
 * carried no DKIM, and failed DMARC against a published `p=quarantine`. The
 * pipeline caught all of it — `sender_trust: suspicious`, `category: spam` —
 * and the reading pane still drew it with the recipient's own avatar.
 *
 * So the address is necessary and not sufficient: a message is yours when it
 * says so **and** its sender authentication does not contradict it.
 */

/** Verdicts from `mailrs_inbound::sender_trust`. */
export type SenderTrust = 'suspicious' | 'unverified' | 'verified' | string

/**
 * Whether to render a message as your own.
 *
 * Requires both the address and the absence of a failed verdict. A genuine
 * copy of something you sent is written locally and never carries
 * `suspicious`; one that arrived from outside wearing your address does.
 */
export function isOwnMessage(
  senderEmail: string,
  myEmail: string,
  trust: null | SenderTrust | undefined
): boolean {
  if (!senderEmail || !myEmail) return false
  if (senderEmail.toLowerCase() !== myEmail.toLowerCase()) return false
  // Claimed to be you and failed authentication: not you.
  return !isSpoofSuspected(trust)
}

/**
 * Sender authentication actively says this message is not what it claims.
 *
 * Only `suspicious` counts. `unverified` is the vast ordinary middle — most
 * legitimate mail from small senders lands there — and treating it as a
 * warning would train people to ignore the one verdict that matters.
 */
export function isSpoofSuspected(trust: null | SenderTrust | undefined): boolean {
  return trust === 'suspicious'
}
