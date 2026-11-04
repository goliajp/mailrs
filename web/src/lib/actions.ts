import type { Key } from '@/i18n'
// Every action on a conversation, defined once. The context menu, the
// row's hover buttons, the batch bar, the reading pane's header, the
// command palette and the keyboard all draw from this table, so an action
// is named, drawn, keyed, confirmed and undone the same way wherever it is
// reached. Before it, each of those spelled its own labels and its own
// idea of which actions apply, and `#` deleted without asking while the
// other four asked.
//
// The table itself is `actions.json`, shared with the phone apps: iOS and
// Android each ship a byte-identical copy and test their own action enum
// against it, and `scripts/check-action-parity.sh` compares the three.
// What one surface lacks (the phones' `unlabel` and `release`, which the
// web reaches elsewhere) is said by the entry's `surfaces`, not by leaving
// it out.
import type { LucideIcon } from 'lucide-react'

import {
  AlarmClock,
  AlarmClockOff,
  Archive,
  ArchiveRestore,
  ArchiveX,
  Bell,
  Flag,
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

import table from './actions.json'

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
  | 'report'
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

/** The table's icon names, as lucide spells them. */
export const ICONS: Record<string, LucideIcon> = {
  AlarmClock,
  AlarmClockOff,
  Archive,
  ArchiveRestore,
  ArchiveX,
  Bell,
  Flag,
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
}

type Entry = (typeof table.actions)[number]

/** The entries the web offers: those whose `surfaces` name it. */
export const WEB_ENTRIES: Entry[] = table.actions.filter((e) => e.surfaces.includes('web'))

const onWeb = new Set(WEB_ENTRIES.map((e) => e.id))

function webSpec(e: Entry): ActionSpec {
  return {
    batch: e.batch,
    confirm: e.confirm,
    danger: e.danger,
    done: e.done.web as Key | null,
    icon: ICONS[e.icon.web ?? ''],
    id: e.id as ActionId,
    keys: e.keys,
    label: e.label.web as Key,
    undo: webUndo(e.undo),
  }
}

/** An undo the web cannot offer (the phones' `unlabel`) is none here. */
function webUndo(undo: null | string): ActionId | null {
  if (undo !== null && onWeb.has(undo)) return undo as ActionId
  return null
}

// Every id is checked against the ActionId union and every label and icon
// against the dictionary and ICONS by `actions-from-the-shared-table.test.ts`.
export const ACTIONS = Object.fromEntries(WEB_ENTRIES.map((e) => [e.id, webSpec(e)])) as Record<
  ActionId,
  ActionSpec
>

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
    'report',
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
