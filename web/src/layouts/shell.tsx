import type { ReactNode } from 'react'

import { cn } from '@/lib/cn'

export function Panel({
  center,
  children,
  className,
  width,
}: {
  center?: boolean
  children: ReactNode
  className?: string
  width?: number
}) {
  return (
    <div
      className={cn(
        'flex min-h-0 flex-col overflow-hidden bg-[var(--color-bg-raised)] md:rounded-lg',
        width === 280 && 'w-full shrink-0 md:w-[280px]',
        width === 320 && 'w-full shrink-0 md:w-[320px]',
        width === 360 && 'w-full shrink-0 md:w-[360px]',
        width === 400 && 'w-full shrink-0 md:w-[400px]',
        width === 480 && 'w-full shrink-0 md:w-[480px]',
        !width && 'min-w-0 flex-1',
        center && 'items-center justify-center',
        className
      )}
    >
      {children}
    </div>
  )
}

export function PanelRow({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <div
      className={cn(
        'flex min-h-0 min-w-0 flex-1 gap-0 overflow-hidden md:gap-1.5',
        className
      )}
    >
      {children}
    </div>
  )
}

export function Scroll({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <div className={cn('min-h-0 flex-1 overflow-y-auto', className)}>
      {children}
    </div>
  )
}

export function Shell({
  children,
  sidebar,
  statusBar,
}: {
  children: ReactNode
  sidebar: ReactNode
  statusBar: ReactNode
}) {
  return (
    <div className="fixed inset-0 flex flex-col bg-[var(--color-bg-base)] text-[var(--color-text-primary)]">
      <div className="flex min-h-0 flex-1 gap-0 pt-0 pb-0 pl-0 md:gap-1.5 md:pt-1.5 md:pb-1.5 md:pl-1.5">
        <div className="hidden w-14 shrink-0 md:block">{sidebar}</div>
        <div className="flex min-h-0 min-w-0 flex-1 gap-0 overflow-hidden pr-0 md:gap-1.5 md:pr-3">
          {children}
        </div>
      </div>
      {/* mobile bottom nav */}
      <div className="shrink-0 md:hidden">{sidebar}</div>
      <div className="hidden h-7 shrink-0 md:block">{statusBar}</div>
    </div>
  )
}
