import type { PickedItem, SelectableRow } from '@/lib/list-selection'
import type { WireSendStatus } from '@/wire/schemas/sends'

import { atom } from 'jotai'

import { type MailListId, threadAxesOf } from '@/lib/mail-lists'

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
 */

// ── which list, and what is picked in it ─────────────────────────────

/**
 * The list on screen. One value, and the only state behind the tabs —
 * `folderAtom`, `quickFilterAtom` and `showArchivedAtom` below are read
 * off it. See `lib/mail-lists.ts` for why round-tripping through five
 * independent atoms and reconstructing the tab name from them was the
 * shape that let four components disagree about the same thing.
 */
export const activeListAtom = atom<MailListId>('inbox')

/**
 * The row the user clicked, and the list they clicked it in.
 *
 * NOT "the selection" — that is `resolveSelection`, which falls back to
 * the list's first row. This is only the deviation from that default.
 */
export const pickedItemAtom = atom<null | PickedItem>(null)

/**
 * Switch lists. Clearing the pick happens in the same write, so nothing
 * downstream has to observe an intermediate state — which is what the
 * mount and unmount effects in four components were each trying to do.
 */
export const selectMailListAtom = atom(null, (get, set, id: MailListId) => {
  if (get(activeListAtom) === id) return
  set(activeListAtom, id)
  set(pickedItemAtom, null)
})

/** Click a row of the list you are already on. */
export const pickInListAtom = atom(null, (get, set, row: SelectableRow) => {
  set(pickedItemAtom, { ...row, list: get(activeListAtom) })
})

/**
 * Open a specific thread from outside the list — the dashboard, a
 * restored URL. The list is named because a pick only counts inside one.
 */
export const openThreadAtom = atom(null, (_get, set, item: PickedItem) => {
  set(activeListAtom, item.list)
  set(pickedItemAtom, item)
})

/**
 * The status Send narrows itself by — its own axis, which no other list
 * has.
 *
 * The *query* is not here: every list narrows by `searchQueryAtom`, and
 * Send and Draft each having a private one is what made the same search
 * box mean something different depending on which chip was lit. An atom
 * rather than component state because the reading pane has to resolve
 * the same first row the list draws, and it cannot see a `useState`
 * inside `SendList`.
 */
export const sendStatusFilterAtom = atom<null | WireSendStatus>(null)

export const composingNewAtom = atom(false)
export const searchQueryAtom = atom('')
export const categoryFilterAtom = atom<null | string>(null)
export const selectedDomainsAtom = atom<string[]>([])
/**
 * Which connected mailboxes the lists are narrowed to.
 *
 * `null` is every account, and it is not the same as a list holding
 * every id: no filter sends no parameter, which is one less thing for
 * the server to walk. An empty array is somebody who unticked every
 * box, and the honest answer to that is an empty list.
 *
 * The empty string is this deployment's own mail — an account in the
 * filter like the rest, so it can be switched off too.
 */
export const selectedAccountsAtom = atom<null | string[]>(null)
export type MobileView = 'conversation' | 'list' | 'reply' | 'thread'
export const mobileViewAtom = atom<MobileView>('list')

// `relevance` is the order the server returned — exact matches, then
// matches inside a message, then substring hits. It is only meaningful
// while a search is active, and it is the default there: ranking a
// search by date throws away the one thing the ranking knew.
export type SortOrder = 'newest' | 'oldest' | 'relevance' | 'unread'
export const sortOrderAtom = atom<SortOrder>('newest')

// batch selection mode
export const batchModeAtom = atom(false)
export const selectedThreadIdsAtom = atom<Set<string>>(new Set<string>())

// mailbox folder filter — derived from the active list, never set.
//
// Junk is the physical Junk mailbox (set by classifier or "mark junk").
// 'NP' is the merged Notifications & Promotions view (the server reads
// the union of the two buckets). `NonJunk` is not a folder anyone can
// navigate to — it is the scope the Unread and Starred lists ask the
// backend for. Those are attributes of a thread, not places it lives, so
// scoping them to one folder answers a question nobody asked; scoping
// them to everything would drag Junk back out of the one surface it is
// allowed to have.
export type MailFolder = 'Drafts' | 'Inbox' | 'Junk' | 'NonJunk' | 'NP' | 'Sent' | 'Trash' | null
export const folderAtom = atom<MailFolder>(
  (get) => (threadAxesOf(get(activeListAtom))?.folder ?? null) as MailFolder
)

// archived view — derived, same as the folder.
export const showArchivedAtom = atom((get) => threadAxesOf(get(activeListAtom))?.archived === true)

// supermode: mark read across all domain accounts
export const crossAccountReadAtom = atom(false)

// importance section filter: null = all, or 'action' | 'important' | 'other'
export type ImportanceSection = 'important' | 'other' | null
export const importanceSectionAtom = atom<ImportanceSection>(null)

// quick filter — derived, same as the folder. `attachment` has no list
// of its own yet, so nothing produces it; it stays in the union because
// the sticky-unread reset and the row filter both switch on this.
export type QuickFilter = 'all' | 'attachment' | 'starred' | 'unread'
export const quickFilterAtom = atom<QuickFilter>((get) => {
  const axes = threadAxesOf(get(activeListAtom))
  if (axes?.unread) return 'unread'
  if (axes?.starred) return 'starred'
  return 'all'
})

// Threads marked-as-read while the user is sitting on the 'unread' filter.
// They stay visible in the list until the user leaves the unread filter (or
// the chat unmounts), so context isn't yanked out from under them. Gmail
// behaviour. The set is intentionally local to the running session — never
// persisted, never synced to other tabs.
export const stickyUnreadIdsAtom = atom<Set<string>>(new Set<string>())

// keyboard shortcuts dialog
export const shortcutsDialogOpenAtom = atom(false)

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
