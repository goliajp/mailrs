/**
 * Branded id types — layer 1 of the v2.1 architecture (see
 * `.claude/rfcs/20260707-v2.1-webapp-reconstruction.md`).
 *
 * A branded type is a nominal wrapper on top of a primitive. Two
 * distinct `string`-brands cannot be silently swapped even though
 * their runtime representation is identical — the compiler rejects
 * `markThreadRead(accountId)` at the call site.
 *
 * The `Brand<K, T>` trick uses a phantom `__brand` property: it never
 * exists at runtime, but TypeScript treats the two types as
 * incompatible. Casting through this module (`asThreadId(str)`) is
 * the one place we admit the underlying primitive; everywhere else,
 * ids are already branded.
 */

/** Local-part + domain, lowercased. Whatever the auth store hands us. */
export type AccountId = Brand<string, 'AccountId'>

/** Fully-qualified alias address, e.g. `sales@golia.ai`. */
export type AliasAddress = Brand<string, 'AliasAddress'>

/** Domain name, lowercased. e.g. `golia.ai`. */
export type DomainName = Brand<string, 'DomainName'>

/** Draft id, kevy-issued. */
export type DraftId = Brand<string, 'DraftId'>

/** RFC 5322 `Message-ID` header value with angle-brackets stripped. */
export type MessageId = Brand<string, 'MessageId'>

/** Stable per-thread identifier issued by the mailrs core. */
export type ThreadId = Brand<string, 'ThreadId'>

/** Per-account IMAP UID. Numeric wire, branded to keep it out of arithmetic with unrelated ints. */
export type Uid = Brand<number, 'Uid'>

type Brand<K, T extends string> = K & { readonly __brand: T }

// ── narrow constructors ─────────────────────────────────────────────
//
// Every one of these is the ONLY sanctioned way to widen a raw string
// into a branded id. Widening happens at the wire boundary (see
// `web/src/wire/`) — anywhere else it's a smell.

export function asAccountId(raw: string): AccountId {
  if (raw.length === 0) throw new Error('empty AccountId')
  return raw.toLowerCase() as AccountId
}

export function asAliasAddress(raw: string): AliasAddress {
  if (!raw.includes('@')) throw new Error(`bad AliasAddress ${raw}`)
  return raw.toLowerCase() as AliasAddress
}

export function asDomainName(raw: string): DomainName {
  if (raw.length === 0) throw new Error('empty DomainName')
  return raw.toLowerCase() as DomainName
}

export function asDraftId(raw: string): DraftId {
  if (raw.length === 0) throw new Error('empty DraftId')
  return raw as DraftId
}

export function asMessageId(raw: string): MessageId {
  if (raw.length === 0) throw new Error('empty MessageId')
  return raw as MessageId
}

export function asThreadId(raw: string): ThreadId {
  if (raw.length === 0) throw new Error('empty ThreadId')
  return raw as ThreadId
}

export function asUid(raw: number): Uid {
  if (!Number.isInteger(raw) || raw < 0) throw new Error(`bad Uid ${raw}`)
  return raw as Uid
}
