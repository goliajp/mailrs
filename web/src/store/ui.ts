import { atom } from 'jotai'

/**
 * v2.1 phase-5d COMPLETE: `conversationsAtom` / `threadMessagesAtom`
 * / `unreadCountAtom` / `hasMoreAtom` / `loadingMoreAtom` /
 * `initialLoadingAtom` all deleted. The mail-list conversations,
 * thread messages, and their loading flags now live entirely in
 * React Query (`conversationKeys.infinite(filter)` +
 * `mailKeys.thread(threadId)`). Every reader goes through
 * `useFlatConversations` / `useCurrentThreadMessages` /
 * `useCurrentUnreadCount`; every writer goes through
 * `patchAllInfiniteLists` or `reducers/commands/conversation.ts`.
 *
 * Next: rename this file to `store/ui.ts` — everything left below
 * is a genuine local-UI atom.
 */
export const selectedThreadIdAtom = atom<null | string>(null)
export const composingNewAtom = atom(false)
export const searchQueryAtom = atom('')
export const categoryFilterAtom = atom<null | string>(null)
export const selectedDomainsAtom = atom<string[]>([])
export type MobileView = 'conversation' | 'list' | 'reply' | 'thread'
export const mobileViewAtom = atom<MobileView>('list')

export type SortOrder = 'newest' | 'oldest' | 'unread'
export const sortOrderAtom = atom<SortOrder>('newest')

// batch selection mode
export const batchModeAtom = atom(false)
export const selectedThreadIdsAtom = atom<Set<string>>(new Set<string>())

// mailbox folder filter.
// v2.8.2: default flipped from null (mixed by_activity axis — showed
// Junk + Sent threads inside "All") to 'Inbox' (dedicated inbox zset —
// excludes Junk/Sent; starred/archived stay orthogonal flags on top).
// Junk is the physical Junk mailbox (set by classifier or "mark junk").
// v2.9 triage — 'NP' is the merged Notifications & Promotions view
// (backend reads the union of the notifications + promotions folder
// zsets). Notifications and Promotions are distinct buckets underneath.
// `NonJunk` is not a folder anyone can navigate to — it is the scope
// the Unread and Starred views ask the backend for. Those are
// attributes of a thread, not places it lives, so scoping them to one
// folder answers a question nobody asked; scoping them to everything
// would drag Junk back out of the one surface it is allowed to have.
export type MailFolder = 'Drafts' | 'Inbox' | 'Junk' | 'NonJunk' | 'NP' | 'Sent' | 'Trash' | null
export const folderAtom = atom<MailFolder>('Inbox')

// archived view toggle
export const showArchivedAtom = atom(false)

// supermode: mark read across all domain accounts
export const crossAccountReadAtom = atom(false)

// importance section filter: null = all, or 'action' | 'important' | 'other'
export type ImportanceSection = 'important' | 'other' | null
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

// when non-null, the composer opens pre-filled from this saved draft
// (set by the Draft tab, alongside composingNewAtom=true). the composer
// tracks its id so autosave upserts the same draft and send/discard
// deletes it. cleared when the composer closes.
export type ComposeDraftSource = {
  bcc: string
  body: string
  cc: string
  id: number
  /**
   * The conversation this draft is a reply inside, if any.
   *
   * The server has always stored it (`SaveDraftRequest.reply_to_thread_id`)
   * and this type did not read it back, so reopening a reply from the Draft
   * tab produced a compose that still said `Re:` in its subject and had lost
   * every trace of what it was replying to.
   */
  replyToThreadId: null | string
  subject: string
  to: string
}
export const composeDraftSourceAtom = atom<ComposeDraftSource | null>(null)

// when non-null, the composer opens pre-filled from a send that failed
// (set by the Send tab's "Edit and send again", alongside
// composingNewAtom=true). cleared when the composer closes.
//
// `attachments` are descriptions, not files: the bytes never left the
// server. On send, the kept ones are named back by `index` and the server
// re-extracts them from the original envelope — which is the only way a
// 15 MB re-edit costs no transfer and cannot lose its files (RFC
// 20260730-send-status S4 addendum).
export type ComposeRedraftSource = {
  attachments: { content_type: string; filename: string; index: number; size: number }[]
  bcc: string
  body: string
  cc: string
  inReplyTo: null | string
  /** The send this repairs. Sent back so the server knows what to carry. */
  redraftOf: string
  subject: string
  to: string
}
export const composeRedraftSourceAtom = atom<ComposeRedraftSource | null>(null)

// when set, the open thread should scroll to + highlight the message with
// this uid (set by clicking a Sent-view row so its exact outbound message
// is focused). synced to the `?msg=` URL param. cleared once consumed.
export const focusedMessageUidAtom = atom<null | number>(null)
