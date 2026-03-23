import { atom } from 'jotai'

import { setTheme as applyTheme, getTheme, type ThemeMode } from '@/lib/theme'

const baseThemeAtom = atom<ThemeMode>(getTheme())

export const themeAtom = atom(
  (get) => get(baseThemeAtom),
  (_get, set, value: ThemeMode) => {
    applyTheme(value)
    set(baseThemeAtom, value)
  }
)
