/** The sectioned inbox, spread into `en`; kept apart to keep each file small. */
export const sectionsEn = {
  'list.awaiting': 'Waiting',
  'list.awaiting.empty': 'Nothing is waiting for a reply',
  'sections.accept': 'Accept all',
  'sections.accepted': 'Accepted {count} senders',
  'sections.acceptFailed': 'The senders could not be accepted',
  'sections.bar': 'Inbox sections',
  'sections.feed': 'Notifications and promotions',
  'sections.followup': 'Follow up',
  'sections.followup.empty': 'Nothing has waited longer than {days} days',
  'sections.new': 'New senders',
  'sections.new.empty': 'No mail from new senders',
  'sections.other': 'Other',
  'sections.other.empty': 'Nothing else',
  'sections.reply': 'To reply',
  'sections.reply.empty': 'Nothing is waiting for your reply',
  'settings.sections.days': 'Follow up after',
  'settings.sections.daysUnit': 'days',
  'settings.sections.help':
    'Splits the Inbox into mail to reply to, conversations to follow up, mail from new senders and the rest. Waiting means you wrote last and no person has answered; automatic replies do not count',
  'settings.sections.toggle': 'Sectioned inbox',
} as const

export type SectionsKey = keyof typeof sectionsEn
