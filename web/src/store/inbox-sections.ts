import { atom } from 'jotai'

/**
 * The sectioned inbox's settings, per browser like the other display
 * settings: whether the Inbox is shown by section, and after how many days
 * a conversation waiting on a reply is due a follow-up.
 */

const SECTIONED_KEY = 'mailrs_sectioned_inbox'
const FOLLOWUP_DAYS_KEY = 'mailrs_followup_days'

/** Days before a follow-up is due: 3 unless the reader chose otherwise. */
export const DEFAULT_FOLLOWUP_DAYS = 3
export const FOLLOWUP_DAY_CHOICES = [1, 2, 3, 5, 7, 14, 30] as const

function loadFollowupDays(): number {
  const n = Number(localStorage.getItem(FOLLOWUP_DAYS_KEY))
  if (Number.isInteger(n) && n >= 1 && n <= 30) return n
  return DEFAULT_FOLLOWUP_DAYS
}

const baseSectionedAtom = atom<boolean>(localStorage.getItem(SECTIONED_KEY) === 'true')

export const sectionedInboxAtom = atom(
  (get) => get(baseSectionedAtom),
  (_get, set, value: boolean) => {
    localStorage.setItem(SECTIONED_KEY, String(value))
    set(baseSectionedAtom, value)
  }
)

const baseFollowupDaysAtom = atom<number>(loadFollowupDays())

export const followupDaysAtom = atom(
  (get) => get(baseFollowupDaysAtom),
  (_get, set, value: number) => {
    localStorage.setItem(FOLLOWUP_DAYS_KEY, String(value))
    set(baseFollowupDaysAtom, value)
  }
)

/** The part of the sectioned inbox on screen. */
export type InboxSection = 'followup' | 'new' | 'other' | 'reply'

export const INBOX_SECTIONS: readonly InboxSection[] = ['reply', 'followup', 'new', 'other']

export const inboxSectionAtom = atom<InboxSection>('reply')

/**
 * The epoch second a follow-up is due by: `days` before now, rounded down
 * to the hour so the list's cache key does not change on every render.
 */
export function followupDueBy(days: number, nowMs: number): number {
  const hour = Math.floor(nowMs / 3_600_000) * 3600
  return hour - days * 86_400
}
