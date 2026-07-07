/**
 * v2.1 entity-oriented query keys — layer 4 (see RFC §2.4).
 *
 * The old per-screen keys (`mailKeys.conversations` /
 * `dashboardKeys.conversations`) will be deleted once every screen
 * migrates. During the transition this file coexists alongside
 * `query-keys.ts`. **New code MUST import from here.**
 *
 * Rule of thumb: every key starts with the entity name, not the screen.
 * A dashboard reading a conversation list keys on
 * `['conversation','list',filter]` — not on `['dashboard','...']`.
 *
 * The `canonicaliseFilter` helper in `@/domain` guarantees that two
 * calls with equivalent-shape-but-different-key-order filters resolve
 * to the same cache line. Without it, RQ keys off object identity and
 * gets a fresh miss per caller.
 */

import type { ThreadId } from '@/domain'

import { canonicaliseFilter, type ConversationFilter } from '@/domain'

export const conversationKeys = {
  all: () => ['conversation'] as const,

  // Non-paginated list — single-page reads (dashboard, mobile inbox
  // preview, sidebar aggregates). `useConversationList(filter)` keys
  // on this.
  list: (filter?: ConversationFilter) =>
    [...conversationKeys.lists(), canonicaliseFilter(filter)] as const,
  lists: () => [...conversationKeys.all(), 'list'] as const,

  // Paginated infinite list — the `/mail` scroll cursor. RQ stores
  // this shape as `{pages: T[][], pageParams: ...}` and it's NOT
  // interchangeable with a single-page list. Kept as a distinct
  // sub-namespace so a `list` invalidate doesn't blow away all the
  // scroll state, and vice-versa. See RFC §2.4.
  infinite: (filter?: ConversationFilter) =>
    [...conversationKeys.infinites(), canonicaliseFilter(filter)] as const,
  infinites: () => [...conversationKeys.all(), 'infinite'] as const,

  detail: (threadId: ThreadId) => [...conversationKeys.details(), threadId] as const,
  details: () => [...conversationKeys.all(), 'detail'] as const,
} as const

export const messageKeysV21 = {
  all: () => ['message'] as const,
  detail: (uid: number) => [...messageKeysV21.all(), 'detail', uid] as const,
} as const

export const accountKeys = {
  all: () => ['account'] as const,
  detail: (id: string) => [...accountKeys.all(), 'detail', id] as const,
  me: () => [...accountKeys.all(), 'me'] as const,
} as const

/**
 * Admin resources — one namespace per resource type. Reads / writes on
 * `admin.domain` don't invalidate `admin.alias` etc.
 */
export const adminKeysV21 = {
  account: {
    all: () => [...adminKeysV21.all(), 'account'] as const,
    list: () => [...adminKeysV21.account.all(), 'list'] as const,
  },
  alias: {
    all: () => [...adminKeysV21.all(), 'alias'] as const,
    list: () => [...adminKeysV21.alias.all(), 'list'] as const,
  },
  app: {
    all: () => [...adminKeysV21.all(), 'app'] as const,
    list: () => [...adminKeysV21.app.all(), 'list'] as const,
  },
  auditLog: {
    all: () => [...adminKeysV21.all(), 'audit-log'] as const,
    list: (limit?: number) => [...adminKeysV21.auditLog.all(), 'list', limit ?? 200] as const,
  },
  domain: {
    all: () => [...adminKeysV21.all(), 'domain'] as const,
    list: () => [...adminKeysV21.domain.all(), 'list'] as const,
  },
  emailGroup: {
    all: () => [...adminKeysV21.all(), 'email-group'] as const,
    list: () => [...adminKeysV21.emailGroup.all(), 'list'] as const,
  },
  greylist: {
    all: () => [...adminKeysV21.all(), 'greylist'] as const,
    list: () => [...adminKeysV21.greylist.all(), 'list'] as const,
  },
  group: {
    all: () => [...adminKeysV21.all(), 'group'] as const,
    list: () => [...adminKeysV21.group.all(), 'list'] as const,
  },
  queue: {
    all: () => [...adminKeysV21.all(), 'queue'] as const,
    list: () => [...adminKeysV21.queue.all(), 'list'] as const,
  },
  systemConfig: {
    all: () => [...adminKeysV21.all(), 'system-config'] as const,
    list: () => [...adminKeysV21.systemConfig.all(), 'list'] as const,
  },
  all: () => ['admin'] as const,
  health: () => [...adminKeysV21.all(), 'health'] as const,
  smtpConfig: () => [...adminKeysV21.all(), 'smtp-config'] as const,
  status: () => [...adminKeysV21.all(), 'status'] as const,
} as const

export type Snapshot = readonly SnapshotEntry[]
/** Snapshotting helper for optimistic-update rollback (layer 3). */
export type SnapshotEntry = readonly [readonly unknown[], unknown]
