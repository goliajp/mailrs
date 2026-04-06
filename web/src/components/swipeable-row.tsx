import { Archive, Trash2 } from 'lucide-react'
import { useCallback, useRef, useState } from 'react'

const THRESHOLD = 80
const MAX_SWIPE = 120

type SwipeableRowProps = {
  children: React.ReactNode
  onSwipeLeft?: () => void
  onSwipeRight?: () => void
}

// swipeable list row for mobile: right = archive, left = delete
// on md+ breakpoint: renders children directly without gesture layer
export function SwipeableRow({ children, onSwipeLeft, onSwipeRight }: SwipeableRowProps) {
  const [offsetX, setOffsetX] = useState(0)
  const [transitioning, setTransitioning] = useState(false)
  const startX = useRef(0)
  const startY = useRef(0)
  const tracking = useRef(false)
  const directionLocked = useRef(false)

  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    const touch = e.touches[0]
    startX.current = touch.clientX
    startY.current = touch.clientY
    tracking.current = true
    directionLocked.current = false
    setTransitioning(false)
  }, [])

  const handleTouchMove = useCallback(
    (e: React.TouchEvent) => {
      if (!tracking.current) return
      const touch = e.touches[0]
      const dx = touch.clientX - startX.current
      const dy = touch.clientY - startY.current

      // lock direction on first significant move
      if (!directionLocked.current && (Math.abs(dx) > 10 || Math.abs(dy) > 10)) {
        directionLocked.current = true
        // if vertical > horizontal, cancel swipe
        if (Math.abs(dy) > Math.abs(dx)) {
          tracking.current = false
          return
        }
        // ignore swipes starting from screen edge (browser back/forward)
        if (startX.current < 30 || startX.current > window.innerWidth - 30) {
          tracking.current = false
          return
        }
      }

      if (directionLocked.current) {
        // clamp swipe distance
        const clamped = Math.max(-MAX_SWIPE, Math.min(MAX_SWIPE, dx))
        // only allow right swipe if handler exists, same for left
        if ((clamped > 0 && onSwipeRight) || (clamped < 0 && onSwipeLeft)) {
          setOffsetX(clamped)
        }
      }
    },
    [onSwipeLeft, onSwipeRight]
  )

  const handleTouchEnd = useCallback(() => {
    tracking.current = false
    setTransitioning(true)

    if (offsetX >= THRESHOLD && onSwipeRight) {
      onSwipeRight()
    } else if (offsetX <= -THRESHOLD && onSwipeLeft) {
      onSwipeLeft()
    }

    setOffsetX(0)
  }, [offsetX, onSwipeLeft, onSwipeRight])

  const absOffset = Math.abs(offsetX)
  const committed = absOffset >= THRESHOLD

  return (
    <div className="relative overflow-hidden md:contents">
      {/* right swipe background: archive */}
      {offsetX > 0 && (
        <div
          className={`absolute inset-y-0 left-0 flex items-center pl-4 ${committed ? 'bg-success' : 'bg-success/60'}`}
          style={{ width: absOffset }}
        >
          <Archive className="h-5 w-5 text-white" />
        </div>
      )}
      {/* left swipe background: delete */}
      {offsetX < 0 && (
        <div
          className={`absolute inset-y-0 right-0 flex items-center justify-end pr-4 ${committed ? 'bg-danger' : 'bg-danger/60'}`}
          style={{ width: absOffset }}
        >
          <Trash2 className="h-5 w-5 text-white" />
        </div>
      )}
      <div
        className="relative md:!transform-none"
        onTouchEnd={handleTouchEnd}
        onTouchMove={handleTouchMove}
        onTouchStart={handleTouchStart}
        style={{
          transform: `translateX(${offsetX}px)`,
          transition: transitioning ? 'transform 200ms ease-out' : 'none',
        }}
      >
        {children}
      </div>
    </div>
  )
}
