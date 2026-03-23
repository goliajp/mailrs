import type { ColorIntent } from '@/lib/tokens'

type Props = {
  intent?: ColorIntent | 'secondary'
  children: React.ReactNode
  className?: string
}

const intentStyles: Record<string, string> = {
  primary: 'badge-primary bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]',
  secondary: 'badge-secondary bg-[var(--color-bg-sunken)] text-[var(--color-text-secondary)]',
  success:
    'badge-success bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]',
  warning:
    'badge-warning bg-[var(--color-status-warning-subtle)] text-[var(--color-status-warning)]',
  danger: 'badge-danger bg-[var(--color-status-danger-subtle)] text-[var(--color-status-danger)]',
  info: 'badge-info bg-[var(--color-status-info-subtle)] text-[var(--color-status-info)]',
}

export function Badge({ intent = 'secondary', children, className = '' }: Props) {
  return (
    <span
      className={`inline-flex select-none items-center px-1.5 py-0.5 text-[11px] font-medium ${intentStyles[intent] ?? intentStyles.secondary} ${className}`}
    >
      {children}
    </span>
  )
}
