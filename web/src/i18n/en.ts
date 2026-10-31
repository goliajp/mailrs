/**
 * The source dictionary. `zh.ts` and `ja.ts` must have every key here —
 * their type says so, so a missing one fails `tsc`. Keys are grouped by
 * where they appear; `{name}` is filled in by `t(key, { name })`.
 */
export const en = {
  'lang.name.en': 'English',
  'lang.name.ja': '日本語',
  'lang.name.zh': '简体中文',
  'lang.setting': 'Language',
  'lang.setting.browser': 'Same as the browser',

  'list.archived': 'Archived',
  'list.archived.empty': 'No archived conversations',
  'list.draft': 'Draft',
  'list.draft.empty': 'No drafts',
  'list.inbox': 'Inbox',
  'list.inbox.empty': 'All caught up!',
  'list.junk': 'Junk',
  'list.junk.empty': 'No junk mail',
  'list.label': 'Label',
  'list.label.empty': 'No conversations with this label',
  'list.np': 'N & P',
  'list.np.empty': 'Nothing here',
  'list.quarantine': 'Review',
  'list.quarantine.empty': 'Nothing held. No one is currently trying to defraud you.',
  'list.search.empty': 'No results found',
  'list.send': 'Send',
  'list.send.empty': 'Nothing sent yet',
  'list.starred': 'Starred',
  'list.starred.empty': 'Nothing starred',
  'list.tabs': 'Mail lists',
  'list.trash': 'Trash',
  'list.trash.empty': 'Trash is empty',
  'list.unread': 'Unread',
  'list.unread.empty': 'All caught up!',

  'live.newMail': 'New mail from {sender}: {subject}',

  'nav.admin': 'Admin',
  'nav.home': 'Home',
  'nav.mail': 'Mail',
  'nav.mock': 'Mock mail',
  'nav.monitor': 'Monitor',
  'nav.server': 'Server',
  'nav.settings': 'Settings',
  'nav.signOut': 'Sign out',
  'nav.signOutAs': 'Sign out ({address})',
  'nav.theme': 'Switch theme, current: {theme}',
} as const

export type Key = keyof typeof en
