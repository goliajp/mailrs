// re-export gds theme atom directly — single source of truth
// mailrs components that read/write themeAtom are controlling gds's theme system
import { themeAtom } from '@goliapkg/gds'
import { atom } from 'jotai'

type ThemeMode = 'dark' | 'light' | 'system'

// derived atom: read/write only the mode field of gds ThemeState
export const themeModeAtom = atom(
  (get) => get(themeAtom).mode,
  (get, set, mode: ThemeMode) => {
    set(themeAtom, { ...get(themeAtom), mode })
  }
)

// backward compat alias
export const themeAtom_ = themeAtom
export type { ThemeMode }
