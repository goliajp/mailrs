import { forwardRef, type InputHTMLAttributes } from 'react'

type Props = InputHTMLAttributes<HTMLInputElement> & {
  error?: boolean
}

export const Input = forwardRef<HTMLInputElement, Props>(
  ({ className = '', error, ...props }, ref) => (
    <input
      className={`w-full border bg-transparent px-3 py-1.5 text-sm text-[var(--color-text-primary)] placeholder-[var(--color-text-tertiary)] transition-colors outline-none focus:border-[var(--color-brand-primary)] disabled:opacity-40 ${
        error
          ? 'input-danger border-[var(--color-status-danger)]'
          : 'border-[var(--color-border-default)]'
      } ${className}`}
      ref={ref}
      {...props}
    />
  )
)

Input.displayName = 'Input'
