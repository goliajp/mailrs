/** Draft tab entries, spread into `en`; kept apart to keep each file small. */
export const draftsEn = {
  'drafts.attachmentsNotIncluded': 'Attachments not included — the original is in Sent: {names}',
  'drafts.delete': 'Delete draft',
  'drafts.deleteFailed': 'Could not delete draft',
  'drafts.loading': 'Loading drafts…',
  'drafts.noMatch': 'No matching drafts',
  'drafts.open': 'Open',
  'drafts.search': 'Search drafts',
  'drafts.searchPlaceholder': 'Search drafts…',
  'drafts.to': 'To: {to}',
  'drafts.withheldSendAt': 'Not sent: was set to go {time}',
} as const

export type DraftsKey = keyof typeof draftsEn
