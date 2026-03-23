import { forwardRef, type ButtonHTMLAttributes } from 'react'

import type { Size } from '@/lib/tokens'

type Props = Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> & {
  label: string
  size?: Size
  children: React.ReactNode
}

const sizeStyles: Record<Size, string> = {
  xs: 'h-5 w-5',
  sm: 'h-6 w-6',
  md: 'h-7 w-7',
  lg: 'h-8 w-8',
}

export const IconButton = forwardRef<HTMLButtonElement, Props>(
  ({ label, size = 'md', className = '', children, ...props }, ref) => (
    <button
      ref={ref}
      aria-label={label}
      className={`inline-flex items-center justify-center text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)] disabled:opacity-40 ${sizeStyles[size]} ${className}`}
      {...props}
    >
      {children}
    </button>
  ),
)

IconButton.displayName = 'IconButton'
