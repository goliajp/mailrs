/// The shortcuts the app advertises.
///
/// Its own file because the dialog is a component and this is data —
/// and because a test reads it to check that every `g` chord it offers
/// is one `chordList` actually handles. The sheet used to advertise
/// `g a` for a "Go to Action" nothing implemented.

import type { ActionId } from '@/lib/actions'

import { ACTIONS } from '@/lib/actions'

export type ShortcutEntry = {
  description: string
  keys: string[]
}

export type ShortcutGroup = {
  shortcuts: ShortcutEntry[]
  title: string
}

export const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    shortcuts: [
      { description: 'Command palette', keys: ['⌘', 'K'] },
      { description: 'Show shortcuts', keys: ['?'] },
    ],
    title: 'Anywhere',
  },
  {
    shortcuts: [
      { description: 'Next conversation', keys: ['j', '↓'] },
      { description: 'Previous conversation', keys: ['k', '↑'] },
      { description: 'Open conversation', keys: ['Enter'] },
      { description: 'Back to list', keys: ['Esc'] },
    ],
    title: 'Navigation',
  },
  {
    shortcuts: [
      { description: 'New conversation', keys: ['c', 'n'] },
      { description: 'Reply', keys: ['r'] },
      { description: 'Forward', keys: ['f'] },
      ...tableShortcuts(),
      { description: 'Select for a batch action', keys: ['x'] },
      { description: 'Undo the last action', keys: ['z'] },
      { description: 'Focus search', keys: ['/'] },
    ],
    title: 'Actions',
  },
  {
    shortcuts: [
      // These four are `chordList` in `use-keyboard-nav`. This list
      // used to advertise `g a` for a "Go to Action" that was never
      // written, and `g s`, which the key switch shadowed with the
      // star binding — pressing it starred the open thread.
      { description: 'Go to Inbox', keys: ['g', 'i'] },
      { description: 'Go to Sent', keys: ['g', 's'] },
      { description: 'Go to Drafts', keys: ['g', 'd'] },
      { description: 'Go to Archived', keys: ['g', 'a'] },
    ],
    title: 'Go to',
  },
]

/// The action table's keys, one line per key: a pair on one key (`e`
/// archives or puts back) is one line naming both.
function tableShortcuts(): ShortcutEntry[] {
  const keyed: ActionId[][] = [
    ['archive', 'unarchive'],
    ['star', 'unstar'],
    ['pin', 'unpin'],
    ['snooze', 'unsnooze'],
    ['mark-junk'],
    ['unread'],
    ['read'],
    ['trash', 'delete-forever'],
  ]
  return keyed.map((ids) => ({
    description: ids.map((id) => ACTIONS[id].label).join(' / '),
    keys: ACTIONS[ids[0] as ActionId].keys,
  }))
}
