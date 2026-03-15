import type { ReactNode } from 'react'

export function Shell({ sidebar, statusBar, children }: {
  sidebar: ReactNode
  statusBar: ReactNode
  children: ReactNode
}) {
  return (
    <div style={{ position: 'fixed', inset: 0, display: 'flex', flexDirection: 'column', background: 'var(--color-bg-base)', color: 'var(--color-text-primary)' }}>
      <div style={{ display: 'flex', flex: 1, minHeight: 0, gap: 6, paddingTop: 6, paddingBottom: 6, paddingLeft: 6, paddingRight: 6 }}>
        <div style={{ width: 56, flexShrink: 0 }}>{sidebar}</div>
        <div style={{ display: 'flex', flex: 1, minWidth: 0, minHeight: 0, gap: 6, overflow: 'hidden' }}>{children}</div>
      </div>
      <div style={{ height: 28, flexShrink: 0 }}>{statusBar}</div>
    </div>
  )
}

export function Panel({ width, children, center }: {
  width?: number
  children: ReactNode
  center?: boolean
}) {
  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      flex: width ? undefined : '1 1 0%',
      width: width ? width : undefined,
      minWidth: width ? undefined : 0,
      minHeight: 0,
      flexShrink: width ? 0 : undefined,
      overflow: 'hidden',
      borderRadius: 8,
      background: 'var(--color-bg-raised)',
      alignItems: center ? 'center' : undefined,
      justifyContent: center ? 'center' : undefined,
    }}>
      {children}
    </div>
  )
}

export function PanelRow({ children }: { children: ReactNode }) {
  return (
    <div style={{ display: 'flex', flex: 1, minWidth: 0, minHeight: 0, gap: 6 }}>
      {children}
    </div>
  )
}

export function Scroll({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div style={{ flex: 1, minHeight: 0, overflowY: 'auto' }} className={className}>
      {children}
    </div>
  )
}
