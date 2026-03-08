import { forwardRef, type InputHTMLAttributes } from 'react'

type Props = InputHTMLAttributes<HTMLInputElement> & {
  error?: boolean
}

export const Input = forwardRef<HTMLInputElement, Props>(
  ({ error, className = '', ...props }, ref) => (
    <input
      ref={ref}
      className={`w-full border bg-transparent px-3 py-1.5 text-sm text-[var(--color-text-primary)] placeholder-[var(--color-text-tertiary)] outline-none transition-colors focus:border-[var(--color-brand-primary)] disabled:opacity-40 ${
        error
          ? 'border-[var(--color-status-danger)] input-danger'
          : 'border-[var(--color-border-default)]'
      } ${className}`}
      {...props}
    />
  ),
)

Input.displayName = 'Input'
