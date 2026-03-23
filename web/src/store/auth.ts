import { atom } from 'jotai'

export type AuthInfo = {
  token: string
  address: string
  display_name: string
  permissions: string[]
  accessible_domains: string[]
}

const STORAGE_KEY = 'mailrs_auth'

function loadAuth(): AuthInfo | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    return JSON.parse(raw) as AuthInfo
  } catch {
    return null
  }
}

function saveAuth(info: AuthInfo | null) {
  if (info) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(info))
  } else {
    localStorage.removeItem(STORAGE_KEY)
  }
}

const baseAuthAtom = atom<AuthInfo | null>(loadAuth())

export const authAtom = atom(
  (get) => get(baseAuthAtom),
  (_get, set, value: AuthInfo | null) => {
    saveAuth(value)
    set(baseAuthAtom, value)
  },
)

export function getToken(): string | null {
  return loadAuth()?.token ?? null
}
