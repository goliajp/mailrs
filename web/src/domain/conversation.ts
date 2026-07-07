/**
 * Conversation / thread domain types — layer 1.
 *
 * The frontend's mental model, deliberately smaller than what the wire
 * offers. Whatever the wire adds tomorrow, screens still work off this
 * shape; the wire layer is responsible for the translation.
 */

import type { ThreadId, Uid } from './ids'

export type Attachment = {
  readonly contentType: string
  readonly filename: string
  /** Index within the message part tree — used for the raw fetch URL. */
  readonly partIndex: number
  readonly sizeBytes: number
}

/**
 * Categories are the "AI-suggested" bucket labels the fastcore
 * `mailrs-intelligence` crate assigns. Union-typed so screens can
 * branch on them without an unchecked cast.
 */
export type Category =
  | 'inbox'
  | 'newsletter'
  | 'notification'
  | 'personal'
  | 'scam'
  | 'spam'
  | 'update'
  | 'work'

/**
 * The list-query filter — canonical shape shared by every conversation
 * list read. `useConversationList(filter)` keys its cache on this
 * object after canonicalization.
 */
export type ConversationFilter = {
  readonly archived?: boolean
  readonly beforeTs?: number
  readonly category?: Category
  readonly domains?: readonly string[]
  readonly folder?: Folder
  readonly limit?: number
  readonly starred?: boolean
  readonly unread?: boolean
}

/**
 * Folder is a small closed union. Screens branch on it — an `enum` or
 * a bare `string` would let any typo through the type checker.
 */
export type Folder = 'ARCHIVE' | 'DRAFTS' | 'INBOX' | 'SENT' | 'SPAM' | 'STARRED' | 'TRASH'

/**
 * The importance score attached by fastcore's classifier. Kept as a
 * literal union so we can render a badge without an integer scale.
 */
export type ImportanceLevel = 'critical' | 'high' | 'low' | 'medium'

/**
 * Fully-hydrated single-message shape, opened when the user selects a
 * thread. Only threads that the user opens are fetched at this depth;
 * lists carry `ThreadSummary` instead.
 */
export type Message = {
  readonly attachments: readonly Attachment[]
  readonly bcc: readonly Participant[]
  readonly cc: readonly Participant[]
  /** Bitfield reflecting IMAP flags — see `FLAG_*` constants in `flags.ts`. */
  readonly flags: number
  readonly htmlBody: null | string
  readonly internalDate: number
  readonly messageId: string
  readonly recipients: readonly Participant[]
  readonly sender: Participant
  readonly subject: string
  readonly textBody: string
  readonly threadId: ThreadId
  readonly uid: Uid
}

/**
 * A single sender / recipient rendered as one participant chip. The
 * wire ships these as MIME encoded-word strings; the wire layer
 * decodes them before they cross this boundary.
 */
export type Participant = {
  readonly address: string
  readonly displayName: string
}

/** Result of a thread-detail read. */
export type Thread = {
  readonly messages: readonly Message[]
  readonly summary: ThreadSummary
  readonly threadId: ThreadId
}

/**
 * The row that renders in every list — dashboard "Recent Activity",
 * mail list, mobile inbox. Everything the row needs to render is on
 * this object; no cross-key joins at the view layer.
 */
export type ThreadSummary = {
  readonly category: Category
  readonly folder: Folder
  readonly importance: ImportanceLevel
  readonly lastDate: number // unix seconds
  readonly messageCount: number
  readonly participants: readonly Participant[]
  readonly pinned: boolean
  readonly requiresAction: boolean
  readonly snippet: string
  readonly snoozedUntil: null | number // unix seconds when it wakes up
  readonly starred: boolean
  readonly subject: string
  readonly threadId: ThreadId
  readonly unreadCount: number
  /**
   * Monotonic version, incremented on every server-side change.
   * Local optimistic patches set it to Number.MAX_SAFE_INTEGER so
   * reconciliation always overrides them.
   */
  readonly version: number
}

// ── constants ───────────────────────────────────────────────────────

/** IMAP flag bitfield positions, matching the fastcore wire encoding. */
export const FLAG_SEEN = 1 << 5
export const FLAG_ANSWERED = 1 << 6
export const FLAG_FLAGGED = 1 << 7

// ── canonicaliser ───────────────────────────────────────────────────

/**
 * Turn a partial filter into the stable object used as a React-Query
 * key. Ordering + defaults matter — two callers with equivalent
 * intent MUST produce the same key. See RFC §2.4.
 */
export function canonicaliseFilter(f: ConversationFilter | undefined): {
  readonly archived: boolean
  readonly beforeTs: null | number
  readonly category: Category | null
  readonly domains: readonly string[]
  readonly folder: Folder | null
  readonly limit: number
  readonly starred: boolean | null
  readonly unread: boolean | null
} {
  const domains = [...(f?.domains ?? [])].sort()
  return {
    archived: f?.archived ?? false,
    beforeTs: f?.beforeTs ?? null,
    category: f?.category ?? null,
    domains,
    folder: f?.folder ?? null,
    limit: f?.limit ?? 50,
    starred: f?.starred ?? null,
    unread: f?.unread ?? null,
  }
}

/** Sentinel version stamped on optimistic patches. */
export const OPTIMISTIC_VERSION = Number.MAX_SAFE_INTEGER
