import { useSetThemeMode, useTheme } from '@goliapkg/gds'
import { atom } from 'jotai'

type ThemeMode = 'dark' | 'light' | 'system'

const baseThemeAtom = atom<ThemeMode>('system')

export const themeAtom = atom(
  (get) => get(baseThemeAtom),
  (_get, set, value: ThemeMode) => {
    set(baseThemeAtom, value)
  }
)

export { useSetThemeMode, useTheme }
export type { ThemeMode }
