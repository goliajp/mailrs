import { atom } from 'jotai'

import { safeStorage } from '@/lib/safe-storage'

export type AuthInfo = {
  accessible_domains: string[]
  address: string
  display_name: string
  permissions: string[]
  token: string
}

const STORAGE_KEY = 'mailrs_auth'

function loadAuth(): AuthInfo | null {
  try {
    const raw = safeStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    return JSON.parse(raw) as AuthInfo
  } catch {
    return null
  }
}

function saveAuth(info: AuthInfo | null) {
  if (info) {
    safeStorage.setItem(STORAGE_KEY, JSON.stringify(info))
  } else {
    safeStorage.removeItem(STORAGE_KEY)
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
