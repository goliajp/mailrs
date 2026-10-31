/**
 * Arrow keys move focus among a group's items — a tab list, a menu —
 * and Home / End go to the ends, wrapping around. The items are the
 * group's elements matching `selector`, in document order.
 */
export function rovingKeyDown(
  e: React.KeyboardEvent<HTMLElement>,
  selector: string,
  orientation: 'horizontal' | 'vertical'
) {
  const next = orientation === 'horizontal' ? 'ArrowRight' : 'ArrowDown'
  const prev = orientation === 'horizontal' ? 'ArrowLeft' : 'ArrowUp'
  if (!['End', 'Home', next, prev].includes(e.key)) return
  const items = Array.from(e.currentTarget.querySelectorAll<HTMLElement>(selector))
  if (items.length === 0) return
  const at = items.indexOf(document.activeElement as HTMLElement)
  let to = 0
  if (e.key === 'End') to = items.length - 1
  else if (e.key === next) to = (at + 1) % items.length
  else if (e.key === prev) to = (at - 1 + items.length) % items.length
  e.preventDefault()
  items[to].focus()
}
