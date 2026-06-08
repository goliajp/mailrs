import { useCallback, useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'

import { useVisualViewport } from '@/hooks/use-visual-viewport'

type MobileModalProps = {
  children: React.ReactNode
  className?: string
  onClose: () => void
  open: boolean
}

// keyboard-aware modal overlay with focus trap
// on mobile: adjusts height when virtual keyboard appears
// on desktop: standard fixed inset-0 overlay
export function MobileModal({ children, className, onClose, open }: MobileModalProps) {
  const { isKeyboardOpen, keyboardHeight } = useVisualViewport()
  const overlayRef = useRef<HTMLDivElement>(null)
  const previousFocus = useRef<HTMLElement | null>(null)

  // focus trap: capture previous focus and restore on close
  useEffect(() => {
    if (!open) return

    previousFocus.current = document.activeElement as HTMLElement
    const overlay = overlayRef.current
    if (!overlay) return

    // focus first focusable element inside modal
    const focusable = overlay.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    )
    if (focusable.length > 0) {
      focusable[0].focus()
    }

    // trap focus within modal
    const handleTab = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose()
        return
      }
      if (e.key !== 'Tab') return
      if (focusable.length === 0) return

      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
    }

    document.addEventListener('keydown', handleTab)
    return () => {
      document.removeEventListener('keydown', handleTab)
      previousFocus.current?.focus()
    }
  }, [open, onClose])

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose()
    },
    [onClose]
  )

  if (!open) return null

  const style: React.CSSProperties = isKeyboardOpen
    ? { bottom: keyboardHeight, overscrollBehavior: 'contain' }
    : { overscrollBehavior: 'contain' }

  // Portal to body so `fixed inset-0` is anchored to the viewport, not
  // to whatever transformed ancestor (virtualized row, swipeable row,
  // dropdown shell) happens to contain the trigger.
  return createPortal(
    <div
      aria-modal="true"
      className={`fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm ${className ?? ''}`}
      onClick={handleOverlayClick}
      ref={overlayRef}
      role="dialog"
      style={style}
    >
      {children}
    </div>,
    document.body
  )
}
