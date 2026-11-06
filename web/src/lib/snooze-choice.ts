import type { Key } from '@/i18n'

/**
 * Putting a conversation away until later: the four times iOS offers
 * (`ios/Mailrs/Wire/SnoozeChoice.swift`), worked out the same way.
 *
 * "Morning" is 8:00 on the reader's own clock, so it is set as a local
 * wall-clock time and a clock change in between does not move it.
 */
export type SnoozeChoice = 'laterToday' | 'nextWeek' | 'thisWeekend' | 'tomorrowMorning'

export const SNOOZE_CHOICES: { choice: SnoozeChoice; label: Key }[] = [
  { choice: 'laterToday', label: 'snooze.laterToday' },
  { choice: 'tomorrowMorning', label: 'snooze.tomorrowMorning' },
  { choice: 'thisWeekend', label: 'snooze.thisWeekend' },
  { choice: 'nextWeek', label: 'snooze.nextWeek' },
]

const SATURDAY = 6
const MONDAY = 1

export function snoozeDate(choice: SnoozeChoice, now: Date): Date {
  switch (choice) {
    case 'laterToday':
      // three hours on, not an evening clock time: chosen at 11pm there is
      // no evening left, and rolling to tomorrow is a different promise
      return new Date(now.getTime() + 3 * 3600 * 1000)
    case 'nextWeek':
      return morningOf(nextWeekday(MONDAY, now))
    case 'thisWeekend':
      return morningOf(nextWeekday(SATURDAY, now))
    case 'tomorrowMorning':
      return morningOf(dayAfter(now, 1))
  }
}

/** Epoch seconds, what the snooze handler takes. */
export function snoozeSeconds(choice: SnoozeChoice, now: Date): number {
  return Math.floor(snoozeDate(choice, now).getTime() / 1000)
}

function dayAfter(day: Date, n: number): Date {
  const next = new Date(day)
  next.setDate(next.getDate() + n)
  return next
}

function morningOf(day: Date): Date {
  const morning = new Date(day)
  morning.setHours(8, 0, 0, 0)
  return morning
}

/** The next such weekday strictly after `now`: "next week" said on a Monday is the Monday coming. */
function nextWeekday(weekday: number, now: Date): Date {
  const ahead = (weekday - now.getDay() + 7) % 7
  if (ahead === 0) return dayAfter(now, 7)
  return dayAfter(now, ahead)
}
