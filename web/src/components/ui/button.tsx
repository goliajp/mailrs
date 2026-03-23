import type { Size } from '@/lib/tokens'

import { type ButtonHTMLAttributes, forwardRef } from 'react'

export type ButtonVariant = 'danger' | 'ghost' | 'primary' | 'secondary'

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  size?: Size
  variant?: ButtonVariant
}

const variantStyles: Record<ButtonVariant, string> = {
  danger:
    'bg-[var(--color-status-danger)] text-white hover:opacity-90 disabled:opacity-40',
  ghost:
    'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-40',
  primary:
    'bg-[var(--color-brand-primary)] text-[var(--color-brand-primary-text)] hover:bg-[var(--color-brand-primary-hover)] disabled:opacity-40',
  secondary:
    'border border-[var(--color-border-default)] text-[var(--color-text-primary)] hover:bg-[var(--color-hover)] disabled:opacity-40',
}

const sizeStyles: Record<Size, string> = {
  lg: 'h-10 px-4 text-sm gap-2',
  md: 'h-8 px-3 text-sm gap-1.5',
  sm: 'h-7 px-2.5 text-xs gap-1.5',
  xs: 'h-6 px-2 text-[11px] gap-1',
}

export const Button = forwardRef<HTMLButtonElement, Props>(
  (
    { children, className = '', size = 'md', variant = 'secondary', ...props },
    ref
  ) => (
    <button
      className={`inline-flex items-center justify-center font-medium transition-colors focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)] focus-visible:outline-none ${variantStyles[variant]} ${sizeStyles[size]} ${className}`}
      ref={ref}
      {...props}
    >
      {children}
    </button>
  )
)

Button.displayName = 'Button'
