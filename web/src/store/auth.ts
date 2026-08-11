import { atom } from 'jotai'

export type AuthInfo = {
  accessible_domains: string[]
  address: string
  display_name: string
  permissions: string[]
  token: string
}

const STORAGE_KEY = 'mailrs_auth'

function loadAuth(): AuthInfo | null {
  const raw = localStorage.getItem(STORAGE_KEY)
  if (!raw) return null
  try {
    return JSON.parse(raw) as AuthInfo
  } catch {
    // A blob that will not parse is not a session. Returning null signs
    // the reader out, which is recoverable; throwing here happens at
    // module scope and takes the whole app down before it renders.
    localStorage.removeItem(STORAGE_KEY)
    return null
  }
}

/**
 * The session is the one thing that must fit.
 *
 * `localStorage.setItem` throws `QuotaExceededError` when the origin's
 * storage is full, and on 2026-08-11 it did: React Query's persister
 * had been writing a fresh snapshot under a new key on every deploy
 * and deleting none of them. Signing in answered 200 and then failed
 * here, and the login page reported "Network error" — the message its
 * catch-all uses for anything it does not recognise.
 *
 * A few hundred bytes of session must never lose to a cache. When the
 * write fails, the caches go and the write is tried again; only if
 * *that* fails is there nothing more this can do, and then the session
 * lives in memory for the tab rather than the sign-in being refused.
 */
function saveAuth(info: AuthInfo | null) {
  if (!info) {
    localStorage.removeItem(STORAGE_KEY)
    return
  }
  const blob = JSON.stringify(info)
  try {
    localStorage.setItem(STORAGE_KEY, blob)
  } catch {
    for (const key of Object.keys(localStorage)) {
      if (key.startsWith('mailrs:rq:')) localStorage.removeItem(key)
    }
    try {
      localStorage.setItem(STORAGE_KEY, blob)
    } catch {
      // Storage is full of something this app did not put there.
      // Staying signed in for this tab is better than refusing a
      // sign-in that the server accepted.
    }
  }
}

const baseAuthAtom = atom<AuthInfo | null>(loadAuth())

export const authAtom = atom(
  (get) => get(baseAuthAtom),
  (_get, set, value: AuthInfo | null) => {
    saveAuth(value)
    set(baseAuthAtom, value)
  }
)

export function getToken(): null | string {
  return loadAuth()?.token ?? null
}
