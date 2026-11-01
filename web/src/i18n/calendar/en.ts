/** Calendar page entries, spread into `en`; kept apart to keep each file small. */
export const calendarEn = {
  'calendar.answer': 'Your answer',
  'calendar.from': 'From',
  'calendar.loadFailed': 'The calendar could not be loaded',
  'calendar.month': 'Month',
  'calendar.more': '+{n} more',
  'calendar.next': 'Next',
  'calendar.noEvents': 'No events in this period',
  'calendar.organizer': 'Organizer',
  'calendar.previous': 'Previous',
  'calendar.source.feed': 'Subscribed calendar',
  'calendar.source.mail': 'Invitation in your mail',
  'calendar.source.own': 'Your calendar',
  'calendar.status': 'Status',
  'calendar.title': 'Calendar',
  'calendar.today': 'Today',
  'calendar.week': 'Week',
} as const

export type CalendarKey = keyof typeof calendarEn
