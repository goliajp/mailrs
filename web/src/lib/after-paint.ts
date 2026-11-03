/**
 * Run `fn` once the next frame has been painted; returns a cancel.
 *
 * For work an input causes but its result does not need: React runs the
 * effects of a click or key press before the browser paints, so an effect
 * that starts a mutation with an optimistic update puts that update's
 * re-render in front of the paint the reader is waiting for. The
 * animation frame comes before the paint and the task it queues after it.
 */
export function afterPaint(fn: () => void): () => void {
  let timer: null | ReturnType<typeof setTimeout> = null
  const frame = requestAnimationFrame(() => {
    timer = setTimeout(fn, 0)
  })
  return () => {
    cancelAnimationFrame(frame)
    if (timer !== null) clearTimeout(timer)
  }
}
