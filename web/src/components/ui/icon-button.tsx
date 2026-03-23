import type { Size } from '@/lib/tokens'

import { type ButtonHTMLAttributes, forwardRef } from 'react'

type Props = Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> & {
  children: React.ReactNode
  label: string
  size?: Size
}

const sizeStyles: Record<Size, string> = {
  lg: 'h-8 w-8',
  md: 'h-7 w-7',
  sm: 'h-6 w-6',
  xs: 'h-5 w-5',
}

export const IconButton = forwardRef<HTMLButtonElement, Props>(
  ({ children, className = '', label, size = 'md', ...props }, ref) => (
    <button
      aria-label={label}
      className={`inline-flex items-center justify-center text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-primary)] focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)] focus-visible:outline-none disabled:opacity-40 ${sizeStyles[size]} ${className}`}
      ref={ref}
      {...props}
    >
      {children}
    </button>
  )
)

IconButton.displayName = 'IconButton'
