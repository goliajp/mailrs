// design token definitions
// these map to CSS custom properties defined in index.css
// use these constants as the single source of truth for all UI styling

// -- spacing scale (rem) --
export const spacing = {
  0: '0',
  0.5: '0.125rem',
  1: '0.25rem',
  1.5: '0.375rem',
  2: '0.5rem',
  2.5: '0.625rem',
  3: '0.75rem',
  3.5: '0.875rem',
  4: '1rem',
  5: '1.25rem',
  6: '1.5rem',
  8: '2rem',
  10: '2.5rem',
  12: '3rem',
} as const

// -- border radius --
export const radius = {
  full: '9999px',
  lg: '0.5rem',
  md: '0.25rem',
  none: '0',
  sm: '0.125rem',
} as const

// -- font sizes --
export const fontSize = {
  '2xl': '1.25rem', // 20px
  base: '0.875rem', // 14px
  lg: '1rem', // 16px
  sm: '0.8125rem', // 13px
  xl: '1.125rem', // 18px
  xs: '0.6875rem', // 11px
} as const

// -- font weights --
export const fontWeight = {
  bold: '700',
  medium: '500',
  normal: '400',
  semibold: '600',
} as const

// -- z-index layers --
export const zIndex = {
  dropdown: 10,
  modal: 50,
  overlay: 40,
  popover: 60,
  sticky: 20,
  toast: 70,
} as const

// -- semantic color token keys --
// actual values come from CSS custom properties (--color-*)
// these names are used in component variant maps
export type ColorIntent =
  | 'danger'
  | 'info'
  | 'primary'
  | 'secondary'
  | 'success'
  | 'warning'

// -- component size variants --
export type Size = 'lg' | 'md' | 'sm' | 'xs'

export type SurfaceLevel =
  | 'base' // main background
  | 'overlay' // dropdowns, popovers
  | 'raised' // cards, panels
  | 'sunken' // inset areas

// -- CSS variable names for semantic colors --
// these are the actual custom property names set in index.css
export const colorVars = {
  // surfaces
  bgBase: '--color-bg-base',
  bgOverlay: '--color-bg-overlay',
  bgRaised: '--color-bg-raised',
  bgSunken: '--color-bg-sunken',

  // text
  textInverse: '--color-text-inverse',
  textPrimary: '--color-text-primary',
  textSecondary: '--color-text-secondary',
  textTertiary: '--color-text-tertiary',

  // borders
  borderDefault: '--color-border-default',
  borderStrong: '--color-border-strong',

  // brand
  brandPrimary: '--color-brand-primary',
  brandPrimaryHover: '--color-brand-primary-hover',
  brandPrimaryText: '--color-brand-primary-text',

  // status
  statusDanger: '--color-status-danger',
  statusInfo: '--color-status-info',
  statusSuccess: '--color-status-success',
  statusWarning: '--color-status-warning',
} as const

// helper to read a CSS variable value
export function cssVar(name: string): string {
  return `var(${name})`
}
