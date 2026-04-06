import { useCallback, useRef, useState } from 'react'

import { MobileModal } from '@/components/mobile-modal'

type BottomSheetProps = {
  children: React.ReactNode
  onClose: () => void
  open: boolean
}

// mobile: slide-up bottom sheet with drag-to-dismiss
// md+: standard centered modal (delegates to MobileModal)
export function BottomSheet({ children, onClose, open }: BottomSheetProps) {
  const [dragOffset, setDragOffset] = useState(0)
  const [dragging, setDragging] = useState(false)
  const startY = useRef(0)
  const isDragging = useRef(false)

  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    startY.current = e.touches[0].clientY
    isDragging.current = true
    setDragging(true)
  }, [])

  const handleTouchMove = useCallback((e: React.TouchEvent) => {
    if (!isDragging.current) return
    const dy = e.touches[0].clientY - startY.current
    // only allow dragging down
    if (dy > 0) setDragOffset(dy)
  }, [])

  const handleTouchEnd = useCallback(() => {
    isDragging.current = false
    setDragging(false)
    if (dragOffset > 100) {
      onClose()
    }
    setDragOffset(0)
  }, [dragOffset, onClose])

  if (!open) return null

  return (
    <>
      {/* mobile: bottom sheet */}
      <MobileModal className="items-end md:items-center" onClose={onClose} open>
        <div
          className="bg-surface w-full animate-[slideUp_200ms_ease-out] rounded-t-2xl shadow-xl md:mx-4 md:max-w-lg md:rounded-lg md:rounded-t-lg"
          onClick={(e) => e.stopPropagation()}
          style={{
            paddingBottom: 'var(--safe-area-bottom)',
            transform: dragOffset > 0 ? `translateY(${dragOffset}px)` : undefined,
            transition: dragging ? 'none' : 'transform 200ms ease-out',
          }}
        >
          {/* drag handle — mobile only */}
          <div
            className="flex cursor-grab justify-center py-3 active:cursor-grabbing md:hidden"
            onTouchEnd={handleTouchEnd}
            onTouchMove={handleTouchMove}
            onTouchStart={handleTouchStart}
          >
            <div className="bg-border-strong h-1 w-10 rounded-full" />
          </div>
          <div className="px-6 pt-0 pb-6 md:pt-6">{children}</div>
        </div>
      </MobileModal>
    </>
  )
}
