import type { ReactNode } from 'react'
import { cn } from '@/lib/cn'

export function Shell({ sidebar, statusBar, children }: {
  sidebar: ReactNode
  statusBar: ReactNode
  children: ReactNode
}) {
  return (
    <div className="fixed inset-0 flex flex-col bg-[var(--color-bg-base)] text-[var(--color-text-primary)]">
      <div className="flex min-h-0 flex-1 gap-1.5 p-1.5">
        <div className="w-14 shrink-0">{sidebar}</div>
        <div className="flex min-h-0 min-w-0 flex-1 gap-1.5 overflow-hidden">{children}</div>
      </div>
      <div className="h-7 shrink-0">{statusBar}</div>
    </div>
  )
}

export function Panel({ width, children, center, className }: {
  width?: number
  children: ReactNode
  center?: boolean
  className?: string
}) {
  return (
    <div className={cn(
      'flex min-h-0 flex-col overflow-hidden rounded-lg bg-[var(--color-bg-raised)]',
      width === 280 && 'w-[280px] shrink-0',
      width === 320 && 'w-[320px] shrink-0',
      !width && 'min-w-0 flex-1',
      center && 'items-center justify-center',
      className,
    )}>
      {children}
    </div>
  )
}

export function PanelRow({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cn('flex min-h-0 min-w-0 flex-1 gap-1.5 overflow-hidden', className)}>
      {children}
    </div>
  )
}

export function Scroll({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cn('min-h-0 flex-1 overflow-y-auto', className)}>
      {children}
    </div>
  )
}
