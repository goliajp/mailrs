import { useEffect, useState } from 'react'

import { setAnnouncer } from '@/lib/announce'

/**
 * The region `announce` writes into: polite, out of sight. A repeat of the
 * same words is cleared first, or it would not be read again.
 */
export function LiveAnnouncer() {
  const [text, setText] = useState('')
  useEffect(() => {
    setAnnouncer((next) => {
      setText('')
      requestAnimationFrame(() => setText(next))
    })
    return () => setAnnouncer(null)
  }, [])
  return (
    <div
      aria-atomic="true"
      aria-live="polite"
      className="sr-only"
      data-testid="live-announcer"
      role="status"
    >
      {text}
    </div>
  )
}
