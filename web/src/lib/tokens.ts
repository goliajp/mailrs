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
  none: '0',
  sm: '0.125rem',
  md: '0.25rem',
  lg: '0.5rem',
  full: '9999px',
} as const

// -- font sizes --
export const fontSize = {
  xs: '0.6875rem', // 11px
  sm: '0.8125rem', // 13px
  base: '0.875rem', // 14px
  lg: '1rem', // 16px
  xl: '1.125rem', // 18px
  '2xl': '1.25rem', // 20px
} as const

// -- font weights --
export const fontWeight = {
  normal: '400',
  medium: '500',
  semibold: '600',
  bold: '700',
} as const

// -- z-index layers --
export const zIndex = {
  dropdown: 10,
  sticky: 20,
  overlay: 40,
  modal: 50,
  popover: 60,
  toast: 70,
} as const

// -- semantic color token keys --
// actual values come from CSS custom properties (--color-*)
// these names are used in component variant maps
export type ColorIntent = 'primary' | 'secondary' | 'success' | 'warning' | 'danger' | 'info'

export type SurfaceLevel =
  | 'base' // main background
  | 'raised' // cards, panels
  | 'overlay' // dropdowns, popovers
  | 'sunken' // inset areas

// -- component size variants --
export type Size = 'xs' | 'sm' | 'md' | 'lg'

// -- CSS variable names for semantic colors --
// these are the actual custom property names set in index.css
export const colorVars = {
  // surfaces
  bgBase: '--color-bg-base',
  bgRaised: '--color-bg-raised',
  bgOverlay: '--color-bg-overlay',
  bgSunken: '--color-bg-sunken',

  // text
  textPrimary: '--color-text-primary',
  textSecondary: '--color-text-secondary',
  textTertiary: '--color-text-tertiary',
  textInverse: '--color-text-inverse',

  // borders
  borderDefault: '--color-border-default',
  borderStrong: '--color-border-strong',

  // brand
  brandPrimary: '--color-brand-primary',
  brandPrimaryHover: '--color-brand-primary-hover',
  brandPrimaryText: '--color-brand-primary-text',

  // status
  statusSuccess: '--color-status-success',
  statusWarning: '--color-status-warning',
  statusDanger: '--color-status-danger',
  statusInfo: '--color-status-info',
} as const

// helper to read a CSS variable value
export function cssVar(name: string): string {
  return `var(${name})`
}
