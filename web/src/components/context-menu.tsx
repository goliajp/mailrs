import { useCallback, useEffect, useRef, useState } from 'react'

export type ContextMenuItem = {
  danger?: boolean
  icon?: React.ReactNode
  label: string
  onClick: () => void
}

type Position = { x: number; y: number }

export function ContextMenu({
  items,
  onClose,
  position,
}: {
  items: ContextMenuItem[]
  onClose: () => void
  position: null | Position
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
    left: position.x,
    position: 'fixed',
    top: position.y,
    zIndex: 50,
  }

  return (
    <div
      className="border-border bg-surface min-w-[160px] rounded-lg border py-1 shadow-lg"
      ref={ref}
      role="menu"
      style={style}
    >
      {items.map((item) => (
        <button
          className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors ${
            item.danger
              ? 'text-danger hover:bg-danger/10'
              : 'text-fg-secondary hover:bg-bg-secondary'
          }`}
          key={item.label}
          onClick={() => {
            item.onClick()
            onClose()
          }}
          role="menuitem"
        >
          {item.icon && <span className="shrink-0">{item.icon}</span>}
          {item.label}
        </button>
      ))}
    </div>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useContextMenu() {
  const [position, setPosition] = useState<null | Position>(null)

  const open = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setPosition({ x: e.clientX, y: e.clientY })
  }, [])

  const close = useCallback(() => setPosition(null), [])

  return { close, open, position }
}
