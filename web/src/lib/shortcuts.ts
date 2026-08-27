/// The shortcuts the app advertises.
///
/// Its own file because the dialog is a component and this is data —
/// and because a test reads it to check that every `g` chord it offers
/// is one `chordList` actually handles. The sheet used to advertise
/// `g a` for a "Go to Action" nothing implemented.

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
      { description: 'Next conversation', keys: ['j', '↓'] },
      { description: 'Previous conversation', keys: ['k', '↑'] },
      { description: 'Open conversation', keys: ['Enter'] },
      { description: 'Back to list', keys: ['Esc'] },
    ],
    title: 'Navigation',
  },
  {
    shortcuts: [
      { description: 'New conversation', keys: ['n'] },
      { description: 'Reply', keys: ['r'] },
      { description: 'Archive / Unarchive', keys: ['e'] },
      { description: 'Star / Unstar', keys: ['s'] },
      { description: 'Pin / Unpin', keys: ['p'] },
      { description: 'Mark unread', keys: ['u'] },
      { description: 'Mark read + next', keys: ['Shift+I'] },
      { description: 'Forward', keys: ['f'] },
      { description: 'Delete', keys: ['#'] },
      { description: 'Focus search', keys: ['/'] },
      { description: 'Show shortcuts', keys: ['?'] },
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
