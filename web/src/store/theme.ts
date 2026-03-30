// derived atom: read/write only the mode field of gds ThemeState
import { themeAtom } from '@goliapkg/gds'
import { atom } from 'jotai'

type ThemeMode = 'dark' | 'light' | 'system'

export const themeModeAtom = atom(
  (get) => get(themeAtom).mode,
  (get, set, mode: ThemeMode) => {
    set(themeAtom, { ...get(themeAtom), mode })
  }
)

export type { ThemeMode }
