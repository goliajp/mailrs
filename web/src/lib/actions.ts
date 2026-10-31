// Every action on a conversation, defined once. The context menu, the
// row's hover buttons, the batch bar, the reading pane's header, the
// command palette and the keyboard all draw from this table, so an action
// is named, drawn, keyed, confirmed and undone the same way wherever it is
// reached. Before it, each of those spelled its own labels and its own
// idea of which actions apply, and `#` deleted without asking while the
// other four asked.

import type { LucideIcon } from 'lucide-react'

import {
  AlarmClock,
  AlarmClockOff,
  Archive,
  ArchiveRestore,
  Bell,
  Inbox,
  Mail,
  MailOpen,
  Pin,
  PinOff,
  ShieldAlert,
  ShieldCheck,
  ShoppingBag,
  Star,
  StarOff,
  Trash2,
} from 'lucide-react'

export type ActionId =
  | 'archive'
  | 'delete'
  | 'mark-junk'
  | 'mark-not-junk'
  | 'mark-notification'
  | 'mark-promotion'
  | 'move-to-inbox'
  | 'pin'
  | 'read'
  | 'snooze'
  | 'star'
  | 'unarchive'
  | 'unpin'
  | 'unread'
  | 'unsnooze'
  | 'unstar'

export type ActionSpec = {
  /** In batch mode, over every selected conversation. */
  batch: boolean
  /** Asked before it runs: only what cannot be undone. */
  confirm: boolean
  /** Drawn in red. */
  danger: boolean
  /** The toast once it is done, or null for none (a star needs no word). */
  done: null | string
  icon: LucideIcon
  id: ActionId
  /** Gmail's keys where Gmail has one. */
  keys: string[]
  /** The verb on a button or a menu item. */
  label: string
  /** The action that takes it back, offered in the toast and on `z`. */
  undo: ActionId | null
}

const spec = (s: ActionSpec): ActionSpec => s

export const ACTIONS: Record<ActionId, ActionSpec> = {
  archive: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: 'Archived',
    icon: Archive,
    id: 'archive',
    keys: ['e'],
    label: 'Archive',
    undo: 'unarchive',
  }),
  // held for a confirmation until deleting goes to a trash that can be
  // emptied later; it unlinks the files today
  delete: spec({
    batch: true,
    confirm: true,
    danger: true,
    done: 'Deleted',
    icon: Trash2,
    id: 'delete',
    keys: ['#'],
    label: 'Delete',
    undo: null,
  }),
  'mark-junk': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Moved to Junk',
    icon: ShieldAlert,
    id: 'mark-junk',
    keys: ['!'],
    label: 'Mark as junk',
    undo: 'mark-not-junk',
  }),
  'mark-not-junk': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Marked as not junk',
    icon: ShieldCheck,
    id: 'mark-not-junk',
    keys: [],
    label: 'Mark as not junk',
    undo: 'mark-junk',
  }),
  'mark-notification': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Moved to Notifications',
    icon: Bell,
    id: 'mark-notification',
    keys: [],
    label: 'Mark as Notification',
    undo: 'move-to-inbox',
  }),
  'mark-promotion': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Moved to Promotions',
    icon: ShoppingBag,
    id: 'mark-promotion',
    keys: [],
    label: 'Mark as Promotion',
    undo: 'move-to-inbox',
  }),
  'move-to-inbox': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Moved to Inbox',
    icon: Inbox,
    id: 'move-to-inbox',
    keys: [],
    label: 'Move to Inbox',
    undo: null,
  }),
  pin: spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Pinned',
    icon: Pin,
    id: 'pin',
    keys: ['p'],
    label: 'Pin',
    undo: 'unpin',
  }),
  read: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: null,
    icon: MailOpen,
    id: 'read',
    keys: ['Shift+I'],
    label: 'Mark as read',
    undo: 'unread',
  }),
  snooze: spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Snoozed until tomorrow 9:00',
    icon: AlarmClock,
    id: 'snooze',
    keys: ['b'],
    label: 'Snooze until tomorrow',
    undo: 'unsnooze',
  }),
  star: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: null,
    icon: Star,
    id: 'star',
    keys: ['s'],
    label: 'Star',
    undo: 'unstar',
  }),
  unarchive: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: 'Unarchived',
    icon: ArchiveRestore,
    id: 'unarchive',
    keys: ['e'],
    label: 'Unarchive',
    undo: 'archive',
  }),
  unpin: spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Unpinned',
    icon: PinOff,
    id: 'unpin',
    keys: ['p'],
    label: 'Unpin',
    undo: 'pin',
  }),
  unread: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: 'Marked unread',
    icon: Mail,
    id: 'unread',
    keys: ['u', 'Shift+U'],
    label: 'Mark as unread',
    undo: 'read',
  }),
  unsnooze: spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'Back in your inbox',
    icon: AlarmClockOff,
    id: 'unsnooze',
    keys: ['b'],
    label: 'Wake now',
    undo: 'snooze',
  }),
  unstar: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: null,
    icon: StarOff,
    id: 'unstar',
    keys: ['s'],
    label: 'Unstar',
    undo: 'star',
  }),
}

/** What a conversation is now, as far as which actions apply. */
export type RowState = {
  archived: boolean
  flagged: boolean
  pinned: boolean
  snoozed: boolean
  unread: boolean
  /** The list it is seen in: Junk, the merged N & P view, or any other. */
  view: 'junk' | 'np' | 'other'
}

/** The actions for one conversation, in menu order. */
export function rowActions(s: RowState): ActionId[] {
  return [
    toggle(s.unread, 'read', 'unread'),
    toggle(s.flagged, 'unstar', 'star'),
    toggle(s.pinned, 'unpin', 'pin'),
    toggle(s.archived, 'unarchive', 'archive'),
    toggle(s.snoozed, 'unsnooze', 'snooze'),
    ...bucketActions(s.view),
    'delete',
  ]
}

/** Of a pair, the one that applies now. */
export function toggle(on: boolean, ifOn: ActionId, ifOff: ActionId): ActionId {
  if (on) return ifOn
  return ifOff
}

function bucketActions(view: RowState['view']): ActionId[] {
  switch (view) {
    case 'junk':
      return ['mark-not-junk']
    case 'np':
      return ['move-to-inbox', 'mark-junk']
    case 'other':
      return ['mark-notification', 'mark-promotion', 'mark-junk']
  }
}

/** The batch bar's actions, in order. */
export const BATCH_ACTIONS: ActionId[] = (
  ['read', 'unread', 'star', 'archive', 'delete'] as ActionId[]
).filter((id) => ACTIONS[id].batch)

/**
 * The action a key asks for on a conversation in `s`, or null. A key on
 * a pair (`e`, `s`, `p`, `b`) is whichever of the two applies now.
 */
export function actionForKey(key: string, s: RowState): ActionId | null {
  const applicable = rowActions(s)
  return applicable.find((id) => ACTIONS[id].keys.includes(key)) ?? null
}

/** A key event as the table names keys: `e`, `#`, `Shift+I`. */
export function keyName(e: { key: string; shiftKey: boolean }): string {
  if (e.shiftKey && /^[A-Z]$/.test(e.key)) return `Shift+${e.key}`
  return e.key
}

/** A conversation's state as the table reads it. */
export function rowStateOf(
  row: {
    archived: boolean
    flagged: boolean
    pinned: boolean
    snoozed_until?: null | number
    unread_count: number
  },
  view: RowState['view'],
  now: number
): RowState {
  return {
    archived: row.archived,
    flagged: row.flagged,
    pinned: row.pinned,
    snoozed: (row.snoozed_until ?? 0) > now,
    unread: row.unread_count > 0,
    view,
  }
}

/** The view the table distinguishes, from the folder a list shows. */
export function viewOf(folder: null | string): RowState['view'] {
  if (folder === 'Junk') return 'junk'
  if (folder === 'NP') return 'np'
  return 'other'
}
