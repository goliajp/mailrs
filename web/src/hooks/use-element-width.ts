import { type RefObject, useLayoutEffect, useState } from 'react'

/**
 * The laid-out width of `ref` in px, kept current as it resizes; null
 * until it has one. A hidden element measures 0, which is not a width:
 * the last real one is kept.
 */
export function useElementWidth(ref: RefObject<HTMLElement | null>): null | number {
  const [width, setWidth] = useState<null | number>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const measure = () => {
      if (el.clientWidth > 0) setWidth(el.clientWidth)
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    return () => observer.disconnect()
  }, [ref])
  return width
}
