import { useCallback, useEffect, useRef, useState } from 'react'

export type ContextMenuItem = {
  label: string
  icon?: React.ReactNode
  danger?: boolean
  onClick: () => void
}

type Position = { x: number; y: number }

// eslint-disable-next-line react-refresh/only-export-components
export function useContextMenu() {
  const [position, setPosition] = useState<Position | null>(null)

  const open = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setPosition({ x: e.clientX, y: e.clientY })
  }, [])

  const close = useCallback(() => setPosition(null), [])

  return { position, open, close }
}

export function ContextMenu({
  position,
  items,
  onClose,
}: {
  position: Position | null
  items: ContextMenuItem[]
  onClose: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!position) return

    const handleClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose()
      }
    }
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }

    document.addEventListener('mousedown', handleClick)
    document.addEventListener('keydown', handleEsc)
    return () => {
      document.removeEventListener('mousedown', handleClick)
      document.removeEventListener('keydown', handleEsc)
    }
  }, [position, onClose])

  if (!position) return null

  // adjust if menu would go off-screen
  const style: React.CSSProperties = {
    position: 'fixed',
    left: position.x,
    top: position.y,
    zIndex: 50,
  }

  return (
    <div ref={ref} style={style} role="menu" className="min-w-[160px] rounded border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] py-1 shadow-lg">
      {items.map((item) => (
        <button
          key={item.label}
          role="menuitem"
          onClick={() => {
            item.onClick()
            onClose()
          }}
          className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors ${
            item.danger
              ? 'text-[var(--color-status-danger)] hover:bg-[var(--color-status-danger-subtle)]'
              : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
          }`}
        >
          {item.icon && <span className="shrink-0">{item.icon}</span>}
          {item.label}
        </button>
      ))}
    </div>
  )
}
