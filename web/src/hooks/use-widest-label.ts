import { type RefObject, useLayoutEffect, useState } from 'react'

/**
 * The width in px of the widest `span` inside `ref`, each laid out at its
 * natural width; 0 until it has been laid out.
 *
 * The spans are measured rather than the elements around them because
 * those stretch to fill their column: measuring them would feed the
 * column's width back in as the minimum and only ever grow. Measured
 * again whenever the container changes size, which also covers it being
 * hidden at one width and shown at another, and whenever `key` changes
 * (the labels' language).
 */
export function useWidestLabel(ref: RefObject<HTMLElement | null>, key: string): number {
  const [widest, setWidest] = useState(0)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const measure = () => {
      let max = 0
      for (const span of el.querySelectorAll('span')) max = Math.max(max, span.offsetWidth)
      // a hidden container measures 0; keep what was measured while shown
      if (max > 0) setWidest(Math.ceil(max))
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    return () => observer.disconnect()
  }, [ref, key])
  return widest
}
