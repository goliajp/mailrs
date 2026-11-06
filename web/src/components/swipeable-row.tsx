import type { ActionId } from '@/lib/actions'

import { useCallback, useRef, useState } from 'react'

import { useIsMobile } from '@/hooks/use-is-mobile'
import { ACTIONS } from '@/lib/actions'

const THRESHOLD = 80
const MAX_SWIPE = 120

/** What one direction does: the action it runs, drawn with that action's icon. */
export type SwipeSide = { action: ActionId; run: () => void }

type SwipeableRowProps = {
  children: React.ReactNode
  /** A finger moving right to left; none when the setting says nothing. */
  left: null | SwipeSide
  right: null | SwipeSide
}

// swipeable list row for mobile; what each direction does is the user's
// swipe setting. On desktop (md+ = 768px+) returns children unwrapped so
// the surrounding virtualizer measures the real content height directly.
export function SwipeableRow({ children, left, right }: SwipeableRowProps) {
  const onSwipeLeft = left?.run
  const onSwipeRight = right?.run
  const isMobile = useIsMobile()
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

  // desktop short-circuit: no gesture scaffolding, children pass straight
  // through to the surrounding virtualizer's measureElement.
  if (!isMobile) return <>{children}</>

  const absOffset = Math.abs(offsetX)
  const committed = absOffset >= THRESHOLD

  return (
    <div className="relative overflow-hidden">
      {offsetX > 0 && right && (
        <SwipeBackground
          action={right.action}
          committed={committed}
          edge="left"
          width={absOffset}
        />
      )}
      {offsetX < 0 && left && (
        <SwipeBackground
          action={left.action}
          committed={committed}
          edge="right"
          width={absOffset}
        />
      )}
      <div
        className="relative"
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

function edgeClass(edge: 'left' | 'right'): string {
  if (edge === 'left') return 'left-0 pl-4'
  return 'right-0 justify-end pr-4'
}

function SwipeBackground({
  action,
  committed,
  edge,
  width,
}: {
  action: ActionId
  committed: boolean
  edge: 'left' | 'right'
  width: number
}) {
  const Icon = ACTIONS[action].icon
  return (
    <div
      className={`absolute inset-y-0 flex items-center ${edgeClass(edge)} ${toneClass(action, committed)}`}
      data-testid={`swipe-${action}`}
      style={{ width }}
    >
      <Icon className="h-5 w-5 text-white" />
    </div>
  )
}

// archive and restore green, trash red, read and unread the accent; the
// paler shade until the swipe has gone far enough to count. Whole class
// names, so Tailwind finds them in the source.
const TONES = {
  accent: { committed: 'bg-accent', pending: 'bg-accent/60' },
  danger: { committed: 'bg-danger', pending: 'bg-danger/60' },
  success: { committed: 'bg-success', pending: 'bg-success/60' },
}

function toneClass(action: ActionId, committed: boolean): string {
  const tone = TONES[toneOf(action)]
  if (committed) return tone.committed
  return tone.pending
}

function toneOf(action: ActionId): keyof typeof TONES {
  if (action === 'trash') return 'danger'
  if (action === 'read' || action === 'unread') return 'accent'
  return 'success'
}
