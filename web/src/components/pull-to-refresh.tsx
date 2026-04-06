import { useCallback, useRef, useState } from 'react'

const PULL_THRESHOLD = 60
const MAX_PULL = 80

type PullToRefreshProps = {
  children: React.ReactNode
  onRefresh: () => Promise<void> | void
}

// pull-to-refresh container for mobile; no-op on md+ breakpoints
export function PullToRefresh({ children, onRefresh }: PullToRefreshProps) {
  const [pullDistance, setPullDistance] = useState(0)
  const [refreshing, setRefreshing] = useState(false)
  const startY = useRef(0)
  const pulling = useRef(false)
  const containerRef = useRef<HTMLDivElement>(null)

  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    const container = containerRef.current
    if (!container || container.scrollTop > 0) return
    startY.current = e.touches[0].clientY
    pulling.current = true
  }, [])

  const handleTouchMove = useCallback(
    (e: React.TouchEvent) => {
      if (!pulling.current || refreshing) return
      const dy = e.touches[0].clientY - startY.current
      if (dy > 0) {
        // resistance effect
        const distance = Math.min(MAX_PULL, dy * 0.4)
        setPullDistance(distance)
      } else {
        pulling.current = false
        setPullDistance(0)
      }
    },
    [refreshing]
  )

  const handleTouchEnd = useCallback(async () => {
    if (!pulling.current) return
    pulling.current = false

    if (pullDistance >= PULL_THRESHOLD) {
      setRefreshing(true)
      try {
        await onRefresh()
      } finally {
        setRefreshing(false)
      }
    }
    setPullDistance(0)
  }, [pullDistance, onRefresh])

  return (
    <div
      className="relative flex-1 overflow-y-auto md:contents"
      onTouchEnd={handleTouchEnd}
      onTouchMove={handleTouchMove}
      onTouchStart={handleTouchStart}
      ref={containerRef}
    >
      {/* pull indicator */}
      {(pullDistance > 0 || refreshing) && (
        <div
          className="flex items-center justify-center md:hidden"
          style={{ height: refreshing ? 40 : pullDistance }}
        >
          <div
            className={`border-border border-t-accent h-5 w-5 rounded-full border-2 ${refreshing ? 'animate-spin' : ''}`}
            style={refreshing ? undefined : { transform: `rotate(${pullDistance * 4}deg)` }}
          />
        </div>
      )}
      {children}
    </div>
  )
}
