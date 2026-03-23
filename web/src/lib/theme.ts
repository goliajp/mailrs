export type ThemeMode = 'system' | 'light' | 'dark'

const STORAGE_KEY = 'mailrs_theme'

const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')

function applyClass(dark: boolean) {
  if (dark) {
    document.documentElement.classList.add('dark')
  } else {
    document.documentElement.classList.remove('dark')
  }
}

function resolveEffective(mode: ThemeMode): 'light' | 'dark' {
  if (mode === 'system') {
    return mediaQuery.matches ? 'dark' : 'light'
  }
  return mode
}

export function getTheme(): ThemeMode {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (stored === 'light' || stored === 'dark' || stored === 'system') {
    return stored
  }
  return 'system'
}

export function setTheme(mode: ThemeMode) {
  localStorage.setItem(STORAGE_KEY, mode)
  applyClass(resolveEffective(mode) === 'dark')
}

export function initTheme() {
  const mode = getTheme()
  applyClass(resolveEffective(mode) === 'dark')

  mediaQuery.addEventListener('change', () => {
    // only react to system changes when mode is 'system'
    const current = getTheme()
    if (current === 'system') {
      applyClass(mediaQuery.matches)
    }
  })
}
