/**
 * Conversation endpoints — layer 2. Wire → domain adapters.
 *
 * These are the ONLY places in the codebase where a wire shape is
 * lifted into a domain shape. Everything above layer 2 speaks the
 * domain vocabulary.
 */

import {
  asThreadId,
  canonicaliseFilter,
  type Category,
  type ConversationFilter,
  FLAG_FLAGGED,
  FLAG_SEEN,
  type Folder,
  type ImportanceLevel,
  type Message,
  type Participant,
  type ThreadSummary,
} from '@/domain'
import { decodeMimeHeader } from '@/lib/avatar'

import { wireFetch } from '../client'
import { wireThreadListResponseSchema, type WireThreadSummary } from '../schemas/conversation'

// ── list endpoint ────────────────────────────────────────────────────

const FOLDER_VALUES: readonly Folder[] = [
  'INBOX',
  'SENT',
  'DRAFTS',
  'TRASH',
  'SPAM',
  'ARCHIVE',
  'STARRED',
]

const CATEGORY_VALUES: readonly Category[] = [
  'inbox',
  'work',
  'personal',
  'newsletter',
  'spam',
  'scam',
  'notification',
  'update',
]

const IMPORTANCE_VALUES: readonly ImportanceLevel[] = ['low', 'medium', 'high', 'critical']

/**
 * `GET /api/conversations` — the canonical list read. Screens do NOT
 * call this directly; layer 4 wraps it as `useConversationList`.
 */
export async function fetchConversationList(
  filter: ConversationFilter,
  signal?: AbortSignal
): Promise<{ readonly items: readonly ThreadSummary[] }> {
  const canonical = canonicaliseFilter(filter)
  const params = new URLSearchParams()
  params.set('limit', String(canonical.limit))
  if (canonical.folder) params.set('folder', canonical.folder)
  if (canonical.category) params.set('category', canonical.category)
  if (canonical.domains.length > 0) params.set('domains', canonical.domains.join(','))
  if (canonical.archived) params.set('archived', '1')
  if (canonical.trashed) params.set('trashed', '1')
  if (canonical.includeQuarantined) params.set('include_quarantined', '1')
  if (canonical.unread === true) params.set('unread', '1')
  if (canonical.starred === true) params.set('starred', '1')
  if (canonical.beforeTs !== null) params.set('before_ts', String(canonical.beforeTs))

  const raw = await wireFetch(wireThreadListResponseSchema, {
    path: `/conversations?${params.toString()}`,
    signal,
  })
  return { items: raw.items.map((w) => wireSummaryToDomain(w, filter.folder)) }
}

// ── detail endpoint ─────────────────────────────────────────────────

// ── adapters ────────────────────────────────────────────────────────

export function messageIsFlagged(m: Pick<Message, 'flags'>): boolean {
  return (m.flags & FLAG_FLAGGED) !== 0
}

/** Deriving `unreadCount` uses this bit-check so tests can pin it. */
export function messageIsSeen(m: Pick<Message, 'flags'>): boolean {
  return (m.flags & FLAG_SEEN) !== 0
}

function normaliseCategory(raw: string): Category {
  const candidate = raw.toLowerCase() as Category
  if (CATEGORY_VALUES.includes(candidate)) return candidate
  return 'inbox'
}

function normaliseFolder(fromWire?: null | string, fromRequest?: Folder): Folder {
  const candidate = (fromWire ?? '').toUpperCase() as Folder
  if (FOLDER_VALUES.includes(candidate)) return candidate
  return fromRequest ?? 'INBOX'
}

function normaliseImportance(raw?: null | string): ImportanceLevel {
  const candidate = (raw ?? '').toLowerCase() as ImportanceLevel
  if (IMPORTANCE_VALUES.includes(candidate)) return candidate
  return 'low'
}

function participantFromWire(raw: string): Participant {
  const decoded = decodeMimeHeader(raw)
  const angleMatch = decoded.match(/^"?([^"<]*)"?\s*<([^>]+)>$/)
  if (angleMatch) {
    return {
      address: angleMatch[2].trim().toLowerCase(),
      displayName: angleMatch[1].trim(),
    }
  }
  const trimmed = decoded.trim()
  return { address: trimmed.toLowerCase(), displayName: trimmed }
}

function wireSummaryToDomain(w: WireThreadSummary, requestFolder?: Folder): ThreadSummary {
  return {
    category: normaliseCategory(w.category),
    folder: normaliseFolder(w.folder, requestFolder),
    importance: normaliseImportance(w.importance_level),
    lastDate: w.last_date,
    messageCount: w.message_count,
    participants: (w.participants ?? []).map(participantFromWire),
    pinned: w.pinned,
    requiresAction: w.requires_action,
    snippet: w.snippet,
    snoozedUntil: w.snoozed_until ?? null,
    starred: w.flagged,
    subject: decodeMimeHeader(w.subject),
    threadId: asThreadId(w.thread_id),
    unreadCount: w.unread_count,
    version: 0,
  }
}
