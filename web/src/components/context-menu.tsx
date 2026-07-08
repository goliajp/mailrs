import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import { MobileModal } from '@/components/mobile-modal'

export type ContextMenuItem = {
  danger?: boolean
  icon?: React.ReactNode
  label: string
  onClick: () => void
}

type Position = { x: number; y: number }

// mobile: bottom action sheet
export function ActionSheet({
  items,
  onClose,
  open,
}: {
  items: ContextMenuItem[]
  onClose: () => void
  open: boolean
}) {
  if (!open) return null

  return (
    <MobileModal className="items-end md:hidden" onClose={onClose} open>
      <div
        className="bg-surface w-full animate-[slideUp_200ms_ease-out] rounded-t-2xl"
        onClick={(e) => e.stopPropagation()}
        style={{ paddingBottom: 'var(--safe-area-bottom)' }}
      >
        {/* drag handle */}
        <div className="flex justify-center py-3">
          <div className="bg-border-strong h-1 w-10 rounded-full" />
        </div>
        <div className="px-2 pb-2">
          {items.map((item, idx) => (
            <button
              className={`flex w-full items-center gap-3 rounded-lg px-4 py-3 text-left text-sm font-medium transition-colors ${
                item.danger ? 'text-danger active:bg-danger/10' : 'text-fg active:bg-bg-secondary'
              }`}
              key={`${item.label}-${idx}`}
              onClick={() => {
                item.onClick()
                onClose()
              }}
            >
              {item.icon && <span className="shrink-0">{item.icon}</span>}
              {item.label}
            </button>
          ))}
        </div>
        <div className="border-border border-t px-2 pt-1 pb-2">
          <button
            className="text-fg-muted active:bg-bg-secondary w-full rounded-lg px-4 py-3 text-center text-sm font-medium transition-colors"
            onClick={onClose}
          >
            Cancel
          </button>
        </div>
      </div>
    </MobileModal>
  )
}

// desktop: positioned dropdown menu
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

  // Portal to body so the menu escapes any ancestor `transform` (the
  // virtualized list rows + SwipeableRow both apply transforms, which
  // would otherwise re-anchor `position: fixed` to the row instead of
  // the viewport — see CSS containing-block rules).
  const style: React.CSSProperties = {
    left: position.x,
    position: 'fixed',
    top: position.y,
    zIndex: 50,
  }

  return createPortal(
    <div
      className="border-border bg-surface hidden min-w-[160px] rounded-lg border py-1 shadow-lg md:block"
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
    </div>,
    document.body
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useContextMenu() {
  const [position, setPosition] = useState<null | Position>(null)
  const [actionSheetOpen, setActionSheetOpen] = useState(false)
  const longPressTimer = useRef<ReturnType<typeof setTimeout>>(null)
  const touchMoved = useRef(false)

  const open = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setPosition({ x: e.clientX, y: e.clientY })
  }, [])

  const close = useCallback(() => {
    setPosition(null)
    setActionSheetOpen(false)
  }, [])

  // long-press handlers for mobile
  const onTouchStart = useCallback(() => {
    touchMoved.current = false
    longPressTimer.current = setTimeout(() => {
      if (!touchMoved.current) {
        // haptic feedback where supported
        if (navigator.vibrate) navigator.vibrate(10)
        setActionSheetOpen(true)
      }
    }, 500)
  }, [])

  const onTouchMove = useCallback(() => {
    touchMoved.current = true
    if (longPressTimer.current) clearTimeout(longPressTimer.current)
  }, [])

  const onTouchEnd = useCallback(() => {
    if (longPressTimer.current) clearTimeout(longPressTimer.current)
  }, [])

  return {
    actionSheetOpen,
    close,
    onTouchEnd,
    onTouchMove,
    onTouchStart,
    open,
    position,
  }
}
