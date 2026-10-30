import { atom } from 'jotai'

const NOTIFICATIONS_KEY = 'mailrs_notifications'
const NOTIFICATION_SOUND_KEY = 'mailrs_notification_sound'

function loadNotifications(): boolean {
  const raw = localStorage.getItem(NOTIFICATIONS_KEY)
  if (raw === null) return true
  return raw === 'true'
}

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

// --- suspected fraud ---
//
// Whether the ordinary lists hide conversations the receive pipeline
// held as suspected fraud. On by default: an attempt to defraud
// somebody should not be something they run into by accident.
//
// This is not a policy switch. What gets held was decided when the
// mail arrived, and turning this off does not release anything — it
// only asks the lists to show what is being held, marked. The review
// screen shows it either way.

const HIDE_FRAUD_KEY = 'mailrs_hide_suspected_fraud'

function loadHideSuspectedFraud(): boolean {
  const raw = localStorage.getItem(HIDE_FRAUD_KEY)
  if (raw === null) return true
  return raw === 'true'
}

const baseHideSuspectedFraudAtom = atom<boolean>(loadHideSuspectedFraud())

export const hideSuspectedFraudAtom = atom(
  (get) => get(baseHideSuspectedFraudAtom),
  (_get, set, value: boolean) => {
    localStorage.setItem(HIDE_FRAUD_KEY, String(value))
    set(baseHideSuspectedFraudAtom, value)
  }
)
