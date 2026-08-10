import { atom } from 'jotai'

const PAGE_SIZE_KEY = 'mailrs_page_size'
const NOTIFICATIONS_KEY = 'mailrs_notifications'
const NOTIFICATION_SOUND_KEY = 'mailrs_notification_sound'

const DEFAULT_PAGE_SIZE = 50

function loadNotifications(): boolean {
  const raw = localStorage.getItem(NOTIFICATIONS_KEY)
  if (raw === null) return true
  return raw === 'true'
}

function loadPageSize(): number {
  const raw = localStorage.getItem(PAGE_SIZE_KEY)
  if (!raw) return DEFAULT_PAGE_SIZE
  const parsed = parseInt(raw, 10)
  if (isNaN(parsed) || parsed < 10 || parsed > 200) return DEFAULT_PAGE_SIZE
  return parsed
}

const basePageSizeAtom = atom<number>(loadPageSize())

export const pageSizeAtom = atom(
  (get) => get(basePageSizeAtom),
  (_get, set, value: number) => {
    const clamped = Math.max(10, Math.min(200, value))
    localStorage.setItem(PAGE_SIZE_KEY, String(clamped))
    set(basePageSizeAtom, clamped)
  }
)

const baseNotificationsAtom = atom<boolean>(loadNotifications())

export const notificationsAtom = atom(
  (get) => get(baseNotificationsAtom),
  (_get, set, value: boolean) => {
    localStorage.setItem(NOTIFICATIONS_KEY, String(value))
    set(baseNotificationsAtom, value)
  }
)

// --- notification sound ---

function loadNotificationSound(): boolean {
  const raw = localStorage.getItem(NOTIFICATION_SOUND_KEY)
  if (raw === null) return true
  return raw === 'true'
}

const baseNotificationSoundAtom = atom<boolean>(loadNotificationSound())

export const notificationSoundAtom = atom(
  (get) => get(baseNotificationSoundAtom),
  (_get, set, value: boolean) => {
    localStorage.setItem(NOTIFICATION_SOUND_KEY, String(value))
    set(baseNotificationSoundAtom, value)
  }
)

// The signature lives on the server, not here.
//
// Two atoms used to hold it in `localStorage`, read by the composer
// and written by no UI anywhere — so what the composer appended was
// permanently empty, while Settings → Signatures saved one through
// `/api/mail/signatures` that nothing ever read. `useDefaultSignature`
// is the one source now.

// standard email signature separator
const SIG_SEPARATOR = '\n\n-- \n'

export function appendSignature(body: string, signature: string, enabled: boolean): string {
  if (!enabled || !signature.trim()) return body
  return body + SIG_SEPARATOR + signature
}
