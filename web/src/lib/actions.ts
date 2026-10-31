import type { Key } from '@/i18n'
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
  ArchiveX,
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
  Tag,
  Trash2,
  Undo2,
} from 'lucide-react'

export type ActionId =
  | 'archive'
  | 'delete-forever'
  | 'label'
  | 'mark-junk'
  | 'mark-not-junk'
  | 'mark-notification'
  | 'mark-promotion'
  | 'move-to-inbox'
  | 'pin'
  | 'read'
  | 'snooze'
  | 'star'
  | 'trash'
  | 'unarchive'
  | 'unpin'
  | 'unread'
  | 'unsnooze'
  | 'unstar'
  | 'untrash'

export type ActionSpec = {
  /** In batch mode, over every selected conversation. */
  batch: boolean
  /** Asked before it runs: only what cannot be undone. */
  confirm: boolean
  /** Drawn in red. */
  danger: boolean
  /** The toast once it is done, or null for none (a star needs no word). */
  done: Key | null
  icon: LucideIcon
  id: ActionId
  /** Gmail's keys where Gmail has one. */
  keys: string[]
  /** The verb on a button or a menu item. */
  label: Key
  /** The action that takes it back, offered in the toast and on `z`. */
  undo: ActionId | null
}

const spec = (s: ActionSpec): ActionSpec => s

export const ACTIONS: Record<ActionId, ActionSpec> = {
  archive: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: 'action.archive.done',
    icon: Archive,
    id: 'archive',
    keys: ['e'],
    label: 'action.archive',
    undo: 'unarchive',
  }),
  // only in the Trash, and asked first: the files go
  'delete-forever': spec({
    batch: false,
    confirm: true,
    danger: true,
    done: 'action.deleteForever.done',
    icon: ArchiveX,
    id: 'delete-forever',
    keys: ['#'],
    label: 'action.deleteForever',
    undo: null,
  }),
  // opens the label picker; what it changes is said there
  label: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: null,
    icon: Tag,
    id: 'label',
    keys: ['l'],
    label: 'action.label',
    undo: null,
  }),
  'mark-junk': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.markJunk.done',
    icon: ShieldAlert,
    id: 'mark-junk',
    keys: ['!'],
    label: 'action.markJunk',
    undo: 'mark-not-junk',
  }),
  'mark-not-junk': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.markNotJunk.done',
    icon: ShieldCheck,
    id: 'mark-not-junk',
    keys: [],
    label: 'action.markNotJunk',
    undo: 'mark-junk',
  }),
  'mark-notification': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.markNotification.done',
    icon: Bell,
    id: 'mark-notification',
    keys: [],
    label: 'action.markNotification',
    undo: 'move-to-inbox',
  }),
  'mark-promotion': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.markPromotion.done',
    icon: ShoppingBag,
    id: 'mark-promotion',
    keys: [],
    label: 'action.markPromotion',
    undo: 'move-to-inbox',
  }),
  'move-to-inbox': spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.moveToInbox.done',
    icon: Inbox,
    id: 'move-to-inbox',
    keys: [],
    label: 'action.moveToInbox',
    undo: null,
  }),
  pin: spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.pin.done',
    icon: Pin,
    id: 'pin',
    keys: ['p'],
    label: 'action.pin',
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
    label: 'action.read',
    undo: 'unread',
  }),
  snooze: spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.snooze.done',
    icon: AlarmClock,
    id: 'snooze',
    keys: ['b'],
    label: 'action.snooze',
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
    label: 'action.star',
    undo: 'unstar',
  }),
  // not asked: the Trash keeps it for 30 days and `z` brings it back
  trash: spec({
    batch: true,
    confirm: false,
    danger: true,
    done: 'action.trash.done',
    icon: Trash2,
    id: 'trash',
    keys: ['#'],
    label: 'action.trash',
    undo: 'untrash',
  }),
  unarchive: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: 'action.unarchive.done',
    icon: ArchiveRestore,
    id: 'unarchive',
    keys: ['e'],
    label: 'action.unarchive',
    undo: 'archive',
  }),
  unpin: spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.unpin.done',
    icon: PinOff,
    id: 'unpin',
    keys: ['p'],
    label: 'action.unpin',
    undo: 'pin',
  }),
  unread: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: 'action.unread.done',
    icon: Mail,
    id: 'unread',
    keys: ['u', 'Shift+U'],
    label: 'action.unread',
    undo: 'read',
  }),
  unsnooze: spec({
    batch: false,
    confirm: false,
    danger: false,
    done: 'action.unsnooze.done',
    icon: AlarmClockOff,
    id: 'unsnooze',
    keys: ['b'],
    label: 'action.unsnooze',
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
    label: 'action.unstar',
    undo: 'star',
  }),
  untrash: spec({
    batch: true,
    confirm: false,
    danger: false,
    done: 'action.untrash.done',
    icon: Undo2,
    id: 'untrash',
    keys: [],
    label: 'action.untrash',
    undo: 'trash',
  }),
}

/** What a conversation is now, as far as which actions apply. */
export type RowState = {
  archived: boolean
  flagged: boolean
  pinned: boolean
  snoozed: boolean
  unread: boolean
  /** The list it is seen in: Junk, the merged N & P view, the Trash, or any other. */
  view: 'junk' | 'np' | 'other' | 'trash'
}

/**
 * The action a key asks for on a conversation in `s`, or null. A key on
 * a pair (`e`, `s`, `p`, `b`) is whichever of the two applies now.
 */
export function actionForKey(key: string, s: RowState): ActionId | null {
  const applicable = rowActions(s)
  return applicable.find((id) => ACTIONS[id].keys.includes(key)) ?? null
}

/** The batch bar's actions, in order. */
export function batchActions(view: RowState['view']): ActionId[] {
  let ids: ActionId[] = ['read', 'unread', 'star', 'archive', 'label', 'trash']
  if (view === 'trash') ids = ['read', 'unread', 'untrash']
  return ids.filter((id) => ACTIONS[id].batch)
}

/** A key event as the table names keys: `e`, `#`, `Shift+I`. */
export function keyName(e: { key: string; shiftKey: boolean }): string {
  if (e.shiftKey && /^[A-Z]$/.test(e.key)) return `Shift+${e.key}`
  return e.key
}

/** The actions for one conversation, in menu order. */
export function rowActions(s: RowState): ActionId[] {
  // in the Trash a conversation can only come back or go for good
  if (s.view === 'trash') return [toggle(s.unread, 'read', 'unread'), 'untrash', 'delete-forever']
  return [
    toggle(s.unread, 'read', 'unread'),
    toggle(s.flagged, 'unstar', 'star'),
    toggle(s.pinned, 'unpin', 'pin'),
    toggle(s.archived, 'unarchive', 'archive'),
    toggle(s.snoozed, 'unsnooze', 'snooze'),
    ...bucketActions(s.view),
    'label',
    'trash',
  ]
}

/** A conversation's state as the table reads it. */
export function rowStateOf(
  row: {
    archived: boolean
    flagged: boolean
    pinned: boolean
    snoozed_until?: null | number
    trashed_at?: number
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
    // a row in the Trash is one wherever it is seen
    view: trashedView((row.trashed_at ?? 0) > 0, view),
  }
}

/** Of a pair, the one that applies now. */
export function toggle(on: boolean, ifOn: ActionId, ifOff: ActionId): ActionId {
  if (on) return ifOn
  return ifOff
}

/** The view the table distinguishes, from the folder a list shows. */
export function viewOf(folder: null | string): RowState['view'] {
  if (folder === 'Junk') return 'junk'
  if (folder === 'NP') return 'np'
  return 'other'
}

function bucketActions(view: RowState['view']): ActionId[] {
  switch (view) {
    case 'junk':
      return ['mark-not-junk']
    case 'np':
      return ['move-to-inbox', 'mark-junk']
    case 'other':
      return ['mark-notification', 'mark-promotion', 'mark-junk']
    case 'trash':
      return []
  }
}

function trashedView(trashed: boolean, view: RowState['view']): RowState['view'] {
  if (trashed) return 'trash'
  return view
}
