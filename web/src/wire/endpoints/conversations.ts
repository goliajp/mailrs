/**
 * Conversation endpoints — layer 2. Wire → domain adapters.
 *
 * These are the ONLY places in the codebase where a wire shape is
 * lifted into a domain shape. Everything above layer 2 speaks the
 * domain vocabulary.
 */

import { z } from 'zod'

import {
  asThreadId,
  asUid,
  type Attachment,
  canonicaliseFilter,
  type Category,
  type ConversationFilter,
  FLAG_FLAGGED,
  FLAG_SEEN,
  type Folder,
  type ImportanceLevel,
  type Message,
  type Participant,
  type Thread,
  type ThreadSummary,
} from '@/domain'
import { decodeMimeHeader } from '@/lib/avatar'

import { wireFetch } from '../client'
import {
  type WireMessage,
  wireThreadDetailResponseSchema,
  wireThreadListResponseSchema,
  type WireThreadSummary,
} from '../schemas/conversation'

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

/**
 * `GET /api/conversations/{threadId}/messages`
 * The single-thread read used by the reader pane.
 */
export async function fetchThread(threadId: string, signal?: AbortSignal): Promise<Thread> {
  const raw = await wireFetch(wireThreadDetailResponseSchema, {
    path: `/conversations/${encodeURIComponent(threadId)}/messages`,
    signal,
  })
  const messages = raw.items.map((m) => wireMessageToDomain(m, threadId))
  const first = messages[0]
  return {
    messages,
    summary: {
      category: 'inbox',
      folder: 'INBOX',
      importance: 'low',
      lastDate: messages.at(-1)?.internalDate ?? 0,
      messageCount: messages.length,
      participants: first ? [first.sender] : [],
      pinned: false,
      requiresAction: false,
      snippet: '',
      snoozedUntil: null,
      starred: false,
      subject: first?.subject ?? '(no subject)',
      threadId: asThreadId(threadId),
      unreadCount: 0,
      version: 0,
    },
    threadId: asThreadId(threadId),
  }
}

// ── adapters ────────────────────────────────────────────────────────

export function messageIsFlagged(m: Pick<Message, 'flags'>): boolean {
  return (m.flags & FLAG_FLAGGED) !== 0
}

/** Deriving `unreadCount` uses this bit-check so tests can pin it. */
export function messageIsSeen(m: Pick<Message, 'flags'>): boolean {
  return (m.flags & FLAG_SEEN) !== 0
}

function attachmentFromWire(
  w: z.infer<typeof import('../schemas/conversation').wireAttachmentSchema>
): Attachment {
  return {
    contentType: w.content_type,
    filename: decodeMimeHeader(w.filename),
    partIndex: w.index,
    sizeBytes: w.size,
  }
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

function splitAddressList(raw: null | string): string[] {
  if (!raw) return []
  return raw
    .split(/[,;]/)
    .map((s) => s.trim())
    .filter(Boolean)
}

/**
 * v2.1 §7 (2026-07-08): backend now sends `recipients` / `cc` /
 * `bcc` as comma-separated strings, not arrays (matches Rust
 * `ThreadMessageResponse` field types). `thread_id` isn't on the
 * wire — it's URL-scoped in the request path. The caller passes
 * the request path's thread_id explicitly.
 */
function wireMessageToDomain(w: WireMessage, threadId: string): Message {
  return {
    attachments: w.attachments.map(attachmentFromWire),
    bcc: splitAddressList(null).map(participantFromWire),
    cc: splitAddressList(w.cc ?? null).map(participantFromWire),
    flags: w.flags,
    htmlBody: w.html_body ?? null,
    internalDate: w.internal_date,
    messageId: w.message_id,
    recipients: splitAddressList(w.recipients).map(participantFromWire),
    sender: participantFromWire(w.sender),
    subject: decodeMimeHeader(w.subject),
    textBody: w.text_body ?? '',
    threadId: asThreadId(threadId),
    uid: asUid(w.uid),
  }
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
