import type { ConversationSummary } from '@/lib/types'

import { atom } from 'jotai'

/**
 * v2.1 phase-5d: `threadMessagesAtom` deleted — thread-view /
 * MobileThreadView keep messages in component-local `useState`; the
 * three read-only callers (mobile-mail views, reply-box) go through
 * `useCurrentThreadMessages()` (RQ-native).
 *
 * `conversationsAtom` still exists during the transition because
 * `conversation-list.test.tsx` mocks `useFlatConversations` to read
 * from it. When that test migrates to seed the RQ cache directly,
 * this atom deletes too — see the RFC's Phase 5d completion criteria.
 */
export const conversationsAtom = atom<ConversationSummary[]>([])
export const selectedThreadIdAtom = atom<null | string>(null)
export const composingNewAtom = atom(false)
export const searchQueryAtom = atom('')
// v2.1 phase-5d: no production reader / writer left for these.
// They're kept only as a test-bridge seed for two component test
// files (`conversation-list.test.tsx`, `thread-view.test.tsx`) that
// still mock `useFlatConversations` to read from the seeded atom.
// Delete these + the mocks together once those two test files
// migrate to seeding the RQ cache directly.
export const hasMoreAtom = atom(true)
export const loadingMoreAtom = atom(false)
export const initialLoadingAtom = atom(true)
export const categoryFilterAtom = atom<null | string>(null)
export const selectedDomainsAtom = atom<string[]>([])
export type MobileView = 'conversation' | 'list' | 'reply' | 'thread'
export const mobileViewAtom = atom<MobileView>('list')

export type SortOrder = 'newest' | 'oldest' | 'unread'
export const sortOrderAtom = atom<SortOrder>('newest')

// batch selection mode
export const batchModeAtom = atom(false)
export const selectedThreadIdsAtom = atom<Set<string>>(new Set<string>())

// mailbox folder filter (null = INBOX default)
// Junk is the physical Junk mailbox (set by sieve rule or "mark spam" action),
// distinct from the AI-derived "Spam" category filter (categoryFilter='spam').
export type MailFolder = 'Drafts' | 'Junk' | 'Sent' | 'Trash' | null
export const folderAtom = atom<MailFolder>(null)

// archived view toggle
export const showArchivedAtom = atom(false)

// supermode: mark read across all domain accounts
export const crossAccountReadAtom = atom(false)

// importance section filter: null = all, or 'action' | 'important' | 'other'
export type ImportanceSection = 'action' | 'important' | 'other' | null
export const importanceSectionAtom = atom<ImportanceSection>(null)

// quick filter
export type QuickFilter = 'all' | 'attachment' | 'starred' | 'unread'
export const quickFilterAtom = atom<QuickFilter>('all')

// Threads marked-as-read while the user is sitting on the 'unread' filter.
// They stay visible in the list until the user leaves the unread filter (or
// the chat unmounts), so context isn't yanked out from under them. Gmail
// behaviour. The set is intentionally local to the running session — never
// persisted, never synced to other tabs.
export const stickyUnreadIdsAtom = atom<Set<string>>(new Set<string>())

// keyboard shortcuts dialog
export const shortcutsDialogOpenAtom = atom(false)

// visible conversation ids in display order (synced from conversation-list)
export const visibleConversationIdsAtom = atom<string[]>([])

// websocket connection status
export type ConnectionStatus = 'connected' | 'connecting' | 'offline'
export const connectionStatusAtom = atom<ConnectionStatus>('connecting')

// mobile thread view: toggle between email content and conversation timeline
export type MobileThreadTab = 'content' | 'conversation'
export const mobileThreadTabAtom = atom<MobileThreadTab>('content')

// mobile full-screen reply composer
export const mobileReplyOpenAtom = atom(false)

// desktop: collapse the conversation timeline / reply pane on the right.
// initial value is auto-derived from viewport width (collapsed below xl
// breakpoint, ~1280px) so narrow desktops aren't crammed by default. user
// can toggle anytime via the thread header button.
export const timelineCollapsedAtom = atom(typeof window !== 'undefined' && window.innerWidth < 1280)

// when non-null, the full-screen composer (NewConversation) opens pre-filled
// as a reply to this message. set alongside composingNewAtom=true by the
// Reply button; cleared when the composer closes or after send
export type ComposeReplySource = {
  htmlBody: null | string
  internalDate: number
  messageId: string
  sender: string
  subject: string
  textBody: null | string
  threadId: string
  uid: number
}
export const composeReplySourceAtom = atom<ComposeReplySource | null>(null)
