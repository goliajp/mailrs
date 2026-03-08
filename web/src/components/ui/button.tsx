import { forwardRef, type ButtonHTMLAttributes } from 'react'

import type { Size } from '@/lib/tokens'

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger'

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant
  size?: Size
}

const variantStyles: Record<ButtonVariant, string> = {
  primary:
    'bg-[var(--color-brand-primary)] text-[var(--color-brand-primary-text)] hover:bg-[var(--color-brand-primary-hover)] disabled:opacity-40',
  secondary:
    'border border-[var(--color-border-default)] text-[var(--color-text-primary)] hover:bg-[var(--color-hover)] disabled:opacity-40',
  ghost:
    'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-40',
  danger:
    'bg-[var(--color-status-danger)] text-white hover:opacity-90 disabled:opacity-40',
}

const sizeStyles: Record<Size, string> = {
  xs: 'h-6 px-2 text-[11px] gap-1',
  sm: 'h-7 px-2.5 text-xs gap-1.5',
  md: 'h-8 px-3 text-sm gap-1.5',
  lg: 'h-10 px-4 text-sm gap-2',
}

export const Button = forwardRef<HTMLButtonElement, Props>(
  ({ variant = 'secondary', size = 'md', className = '', children, ...props }, ref) => (
    <button
      ref={ref}
      className={`inline-flex items-center justify-center font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)] ${variantStyles[variant]} ${sizeStyles[size]} ${className}`}
      {...props}
    >
      {children}
    </button>
  ),
)

Button.displayName = 'Button'
