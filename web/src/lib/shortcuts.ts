import type { ActionId } from '@/lib/actions'
/// The shortcuts the app advertises.
///
/// Its own file because the dialog is a component and this is data —
/// and because a test reads it to check that every `g` chord it offers
/// is one `chordList` actually handles. The sheet used to advertise
/// `g a` for a "Go to Action" nothing implemented.

import { type Key } from '@/i18n'
import { ACTIONS } from '@/lib/actions'

export type ShortcutEntry = {
  description: Key[]
  keys: string[]
}

export type ShortcutGroup = {
  shortcuts: ShortcutEntry[]
  title: Key
}

export const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    shortcuts: [
      { description: ['home.commandPalette'], keys: ['⌘', 'K'] },
      { description: ['shortcuts.show'], keys: ['?'] },
    ],
    title: 'shortcuts.anywhere',
  },
  {
    shortcuts: [
      { description: ['shortcuts.next'], keys: ['j', '↓'] },
      { description: ['shortcuts.previous'], keys: ['k', '↑'] },
      { description: ['shortcuts.open'], keys: ['Enter'] },
      { description: ['shortcuts.back'], keys: ['Esc'] },
    ],
    title: 'palette.navigation',
  },
  {
    shortcuts: [
      { description: ['list.newConversation'], keys: ['c', 'n'] },
      { description: ['read.reply'], keys: ['r'] },
      { description: ['compose.mode.forward'], keys: ['f'] },
      ...tableShortcuts(),
      { description: ['shortcuts.select'], keys: ['x'] },
      { description: ['shortcuts.undo'], keys: ['z'] },
      { description: ['shortcuts.search'], keys: ['/'] },
    ],
    title: 'palette.actions',
  },
  {
    shortcuts: [
      // These four are `chordList` in `use-keyboard-nav`. This list
      // used to advertise `g a` for a "Go to Action" that was never
      // written, and `g s`, which the key switch shadowed with the
      // star binding — pressing it starred the open thread.
      { description: ['palette.goInbox'], keys: ['g', 'i'] },
      { description: ['palette.goSent'], keys: ['g', 's'] },
      { description: ['shortcuts.goDrafts'], keys: ['g', 'd'] },
      { description: ['shortcuts.goArchived'], keys: ['g', 'a'] },
    ],
    title: 'shortcuts.goTo',
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
    ['label'],
    ['unread'],
    ['read'],
    ['trash', 'delete-forever'],
  ]
  return keyed.map((ids) => ({
    description: ids.map((id) => ACTIONS[id].label),
    keys: ACTIONS[ids[0] as ActionId].keys,
  }))
}
