import type { MailListFilters } from '@/lib/query-keys'

export type MailList = {
  /** What the list says when it has nothing in it. */
  emptyLabel: string
  /** The chip's text. */
  label: string
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
  | 'archived'
  | 'draft'
  | 'inbox'
  | 'junk'
  | 'np'
  | 'send'
  | 'starred'
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
  | { filters: ThreadListAxes; kind: 'threads' }
  | { kind: 'drafts' }
  | { kind: 'sends' }

/** The axes of `MailListFilters` a list fixes. The rest are refinements. */
export type ThreadListAxes = Pick<MailListFilters, 'archived' | 'folder' | 'starred' | 'unread'>

/**
 * `NonJunk` for Unread and Starred is deliberate and not a folder anyone
 * can navigate to: they are attributes of a thread rather than places it
 * lives, so scoping them to one folder answers a question nobody asked,
 * and scoping them to everything drags Junk back out of the one surface
 * it is allowed to have.
 */
export const MAIL_LISTS: Record<MailListId, MailList> = {
  archived: {
    emptyLabel: 'No archived conversations',
    label: 'Archived',
    // Cross-folder: the server drops the folder when this is set, because
    // "archived within Inbox" is not what the tab means.
    selectable: true,
    source: { filters: { archived: true }, kind: 'threads' },
  },
  draft: {
    emptyLabel: 'No drafts',
    label: 'Draft',
    selectable: false,
    source: { kind: 'drafts' },
  },
  inbox: {
    emptyLabel: 'All caught up!',
    label: 'Inbox',
    selectable: true,
    source: { filters: { folder: 'Inbox' }, kind: 'threads' },
  },
  junk: {
    emptyLabel: 'No junk mail',
    label: 'Junk',
    selectable: true,
    source: { filters: { folder: 'Junk' }, kind: 'threads' },
  },
  np: {
    emptyLabel: 'Nothing here',
    label: 'N & P',
    selectable: true,
    source: { filters: { folder: 'NP' }, kind: 'threads' },
  },
  // "Send", not "Sent": the view holds sends that failed and sends still
  // going out, so a heading claiming they were sent would be wrong about
  // the rows it is showing.
  send: {
    emptyLabel: 'Nothing sent yet',
    label: 'Send',
    selectable: true,
    source: { kind: 'sends' },
  },
  starred: {
    emptyLabel: 'Nothing starred',
    label: 'Starred',
    selectable: true,
    source: { filters: { folder: 'NonJunk', starred: true }, kind: 'threads' },
  },
  unread: {
    emptyLabel: 'All caught up!',
    label: 'Unread',
    selectable: true,
    source: { filters: { folder: 'NonJunk', unread: true }, kind: 'threads' },
  },
}

/** The two rows of chips, in the order the filter bar draws them. */
/// The tabs, in the order they are shown.
///
/// Flat, and laid out by a five-column grid rather than by this list
/// being nested: two flex rows sized each tab to its own label, so the
/// second row lined up with nothing. The break after `junk` is what
/// the grid does with five columns, not something declared here.
export const MAIL_LIST_TABS: MailListId[] = [
  'inbox',
  'np',
  'unread',
  'starred',
  'junk',
  'send',
  'draft',
  'archived',
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
