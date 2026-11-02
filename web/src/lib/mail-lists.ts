import type { Key } from '@/i18n'
import type { MailListFilters } from '@/lib/query-keys'

export type MailList = {
  /** What the list says when it has nothing in it. */
  emptyLabel: Key
  /** The chip's text. */
  label: Key
  /**
   * Whether a row of this list can become the current item.
   *
   * Draft is the one that cannot, and it is a property of the list
   * rather than a case in a switch: a draft opens the composer, so
   * auto-selecting the first row would pop it open the moment you
   * arrived at the tab. Every other list selects its first row.
   */
  selectable: boolean
  source: MailListSource
}

/**
 * The lists the mail screen can show, as one value each.
 *
 * "Which list am I looking at" used to be six atoms — `folder`,
 * `quickFilter`, `showArchived`, `categoryFilter`, `importanceSection`
 * and the search box — and `resolveActiveTab()` reconstructed the tab
 * name back out of them to decide which chip to highlight. A fact that
 * has to be reverse-engineered from its own consequences is a fact
 * nobody owns, and four different components each kept their own
 * opinion of what the current list's first row was.
 *
 * So the tab is the state and the filters are derived, not the other way
 * round. Whether a list is a real backend axis (Inbox, Junk) or one the
 * server assembles from a flag (Unread, Starred, Archived) or a
 * different endpoint entirely (Send, Draft) is a fact about the source,
 * not about the UI — the screen treats all eight the same way.
 */
export type MailListId =
  | 'all'
  | 'archived'
  | 'assigned'
  | 'awaiting'
  | 'draft'
  | 'inbox'
  | 'junk'
  | 'label'
  | 'np'
  | 'pinned'
  | 'quarantine'
  | 'send'
  | 'snoozed'
  | 'starred'
  | 'trash'
  | 'unread'

/**
 * Where a list's rows come from.
 *
 * `threads` is `/conversations` with the list's axes; the other two are
 * their own endpoints with their own row shapes. Nothing above this
 * cares which — `useCurrentListRows` dispatches once and everything
 * downstream sees rows.
 */
export type MailListSource =
  { filters: ThreadListAxes; kind: 'threads' } | { kind: 'drafts' } | { kind: 'sends' }

/** The axes of `MailListFilters` a list fixes. The rest are refinements. */
export type ThreadListAxes = Pick<
  MailListFilters,
  | 'archived'
  | 'assignedToMe'
  | 'folder'
  | 'followupDueBy'
  | 'label'
  | 'pinned'
  | 'quarantined'
  | 'section'
  | 'snoozed'
  | 'starred'
  | 'trashed'
  | 'unread'
>

/**
 * `NonJunk` for Unread and Starred is deliberate and not a folder anyone
 * can navigate to: they are attributes of a thread rather than places it
 * lives, so scoping them to one folder answers a question nobody asked,
 * and scoping them to everything drags Junk back out of the one surface
 * it is allowed to have.
 */
export const MAIL_LISTS: Record<MailListId, MailList> = {
  // Gmail's All Mail: everything but Junk and the Trash, archived and
  // sent-only conversations included. The server reads both sides of the
  // archive for this folder.
  all: {
    emptyLabel: 'list.all.empty',
    label: 'list.all',
    selectable: true,
    source: { filters: { folder: 'all' }, kind: 'threads' },
  },
  // Across the Inbox and Sent: the reader wrote last and no person has
  // answered since. Ordered, and paged, by when the wait began.
  archived: {
    emptyLabel: 'list.archived.empty',
    label: 'list.archived',
    // Cross-folder: the server drops the folder when this is set, because
    // "archived within Inbox" is not what the tab means.
    selectable: true,
    source: { filters: { archived: true }, kind: 'threads' },
  },
  // Team inboxes: the conversations a teammate assigned to the reader,
  // across folders, newest first.
  assigned: {
    emptyLabel: 'list.assigned.empty',
    label: 'list.assigned',
    selectable: true,
    source: { filters: { assignedToMe: true }, kind: 'threads' },
  },
  awaiting: {
    emptyLabel: 'list.awaiting.empty',
    label: 'list.awaiting',
    selectable: true,
    source: { filters: { section: 'awaiting' }, kind: 'threads' },
  },
  draft: {
    emptyLabel: 'list.draft.empty',
    label: 'list.draft',
    selectable: false,
    source: { kind: 'drafts' },
  },
  inbox: {
    emptyLabel: 'list.inbox.empty',
    label: 'list.inbox',
    selectable: true,
    source: { filters: { folder: 'Inbox' }, kind: 'threads' },
  },
  junk: {
    emptyLabel: 'list.junk.empty',
    label: 'list.junk',
    selectable: true,
    source: { filters: { folder: 'Junk' }, kind: 'threads' },
  },
  // One label's conversations; which label is `activeLabelAtom`. Not a
  // tab: the labels are the user's own, shown in a row of their own.
  label: {
    emptyLabel: 'list.label.empty',
    label: 'list.label',
    selectable: true,
    source: { filters: {}, kind: 'threads' },
  },
  np: {
    emptyLabel: 'list.np.empty',
    label: 'list.np',
    selectable: true,
    source: { filters: { folder: 'NP' }, kind: 'threads' },
  },
  quarantine: {
    // A real answer, not an apology: nothing is being held.
    emptyLabel: 'list.quarantine.empty',
    label: 'list.quarantine',
    selectable: true,
    // Its own endpoint, not a predicate over the ordinary lists —
    // those exclude held conversations by construction, so a filter
    // over them could not reach these.
    source: { filters: { quarantined: true }, kind: 'threads' },
  },
  // "Send", not "Sent": the view holds sends that failed and sends still
  // going out, so a heading claiming they were sent would be wrong about
  // the rows it is showing.
  pinned: {
    emptyLabel: 'list.pinned.empty',
    label: 'list.pinned',
    selectable: true,
    source: { filters: { folder: 'NonJunk', pinned: true }, kind: 'threads' },
  },
  send: {
    emptyLabel: 'list.send.empty',
    label: 'list.send',
    selectable: true,
    source: { kind: 'sends' },
  },
  // Across folders: put away until a time still to come. Those threads are
  // archived while away, which is why this is not a predicate on Archived.
  snoozed: {
    emptyLabel: 'list.snoozed.empty',
    label: 'list.snoozed',
    selectable: true,
    source: { filters: { snoozed: true }, kind: 'threads' },
  },
  starred: {
    emptyLabel: 'list.starred.empty',
    label: 'list.starred',
    selectable: true,
    source: { filters: { folder: 'NonJunk', starred: true }, kind: 'threads' },
  },
  // Cross-folder like Archived: what this reader deleted, from any list,
  // until the server deletes it for good 30 days later.
  trash: {
    emptyLabel: 'list.trash.empty',
    label: 'list.trash',
    selectable: true,
    source: { filters: { trashed: true }, kind: 'threads' },
  },
  unread: {
    emptyLabel: 'list.unread.empty',
    label: 'list.unread',
    selectable: true,
    source: { filters: { folder: 'NonJunk', unread: true }, kind: 'threads' },
  },
}

/** The two rows of chips, in the order the filter bar draws them. */
/// The tabs, in the order they are shown.
///
/// Flat, and laid out by a grid rather than by this list being nested:
/// two flex rows sized each tab to its own label, so the second row lined
/// up with nothing. Six columns, four on a phone; the folder column on wide
/// screens shows them in the same order.
export const MAIL_LIST_TABS: MailListId[] = [
  'inbox',
  'np',
  'unread',
  'starred',
  'pinned',
  'snoozed',
  'awaiting',
  'assigned',
  'send',
  'draft',
  'all',
  'archived',
  'junk',
  'quarantine',
  'trash',
]

export function isMailListId(v: unknown): v is MailListId {
  return typeof v === 'string' && v in MAIL_LISTS
}

/**
 * The thread axes a list fixes, or nothing when its rows do not come
 * from `/conversations`.
 *
 * Send and Draft still render the filter bar, and the conversation query
 * is not what fills them — so callers that need the axes ask for them
 * rather than reading a folder off a list that has none.
 */
export function threadAxesOf(id: MailListId): null | ThreadListAxes {
  const { source } = MAIL_LISTS[id]
  if (source.kind !== 'threads') return null
  return source.filters
}
