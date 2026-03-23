import type { ColorIntent } from '@/lib/tokens'

type Props = {
  children: React.ReactNode
  className?: string
  intent?: 'secondary' | ColorIntent
}

const intentStyles: Record<string, string> = {
  danger:
    'badge-danger bg-[var(--color-status-danger-subtle)] text-[var(--color-status-danger)]',
  info: 'badge-info bg-[var(--color-status-info-subtle)] text-[var(--color-status-info)]',
  primary:
    'badge-primary bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]',
  secondary:
    'badge-secondary bg-[var(--color-bg-sunken)] text-[var(--color-text-secondary)]',
  success:
    'badge-success bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]',
  warning:
    'badge-warning bg-[var(--color-status-warning-subtle)] text-[var(--color-status-warning)]',
}

export function Badge({
  children,
  className = '',
  intent = 'secondary',
}: Props) {
  return (
    <span
      className={`inline-flex items-center px-1.5 py-0.5 text-[11px] font-medium select-none ${intentStyles[intent] ?? intentStyles.secondary} ${className}`}
    >
      {children}
    </span>
  )
}
