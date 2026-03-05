import { atom } from 'jotai'

const PAGE_SIZE_KEY = 'mailrs_page_size'
const NOTIFICATIONS_KEY = 'mailrs_notifications'
const SIGNATURE_KEY = 'mailrs_signature'
const SIGNATURE_ENABLED_KEY = 'mailrs_signature_enabled'

const DEFAULT_PAGE_SIZE = 50

function loadPageSize(): number {
  try {
    const raw = localStorage.getItem(PAGE_SIZE_KEY)
    if (!raw) return DEFAULT_PAGE_SIZE
    const parsed = parseInt(raw, 10)
    if (isNaN(parsed) || parsed < 10 || parsed > 200) return DEFAULT_PAGE_SIZE
    return parsed
  } catch {
    return DEFAULT_PAGE_SIZE
  }
}

function loadNotifications(): boolean {
  try {
    const raw = localStorage.getItem(NOTIFICATIONS_KEY)
    if (raw === null) return true
    return raw === 'true'
  } catch {
    return true
  }
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

// --- signature ---

function loadSignature(): string {
  try {
    return localStorage.getItem(SIGNATURE_KEY) ?? ''
  } catch {
    return ''
  }
}

function loadSignatureEnabled(): boolean {
  try {
    const raw = localStorage.getItem(SIGNATURE_ENABLED_KEY)
    if (raw === null) return false
    return raw === 'true'
  } catch {
    return false
  }
}

const baseSignatureAtom = atom<string>(loadSignature())

export const signatureAtom = atom(
  (get) => get(baseSignatureAtom),
  (_get, set, value: string) => {
    localStorage.setItem(SIGNATURE_KEY, value)
    set(baseSignatureAtom, value)
  }
)

const baseSignatureEnabledAtom = atom<boolean>(loadSignatureEnabled())

export const signatureEnabledAtom = atom(
  (get) => get(baseSignatureEnabledAtom),
  (_get, set, value: boolean) => {
    localStorage.setItem(SIGNATURE_ENABLED_KEY, String(value))
    set(baseSignatureEnabledAtom, value)
  }
)

// standard email signature separator
const SIG_SEPARATOR = '\n\n-- \n'

export function appendSignature(body: string, signature: string, enabled: boolean): string {
  if (!enabled || !signature.trim()) return body
  return body + SIG_SEPARATOR + signature
}
