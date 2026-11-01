/**
 * Inside the stack the reading keys move between messages: ↑ ↓ j k from one
 * message's header to the next, Home and End to the ends, o (and Enter, the
 * header being a button) to open or fold one. Handled here and not passed
 * on, so the same keys do not also move the list to another conversation.
 */
export function stackKeyDown(e: React.KeyboardEvent<HTMLElement>) {
  const target = e.target as HTMLElement
  if (!target.matches('[data-stack-header]')) return
  const headers = Array.from(e.currentTarget.querySelectorAll<HTMLElement>('[data-stack-header]'))
  const at = headers.indexOf(target)
  const to = nextHeader(e.key, at, headers.length)
  if (to !== null) {
    e.preventDefault()
    e.stopPropagation()
    headers[to]?.focus()
    return
  }
  if (e.key === 'o') {
    e.preventDefault()
    e.stopPropagation()
    target.click()
    return
  }
  // the header's own click; the list must not also take it
  if (e.key === 'Enter') e.stopPropagation()
}

function nextHeader(key: string, at: number, count: number): null | number {
  switch (key) {
    case 'ArrowDown':
    case 'j':
      return Math.min(at + 1, count - 1)
    case 'ArrowUp':
    case 'k':
      return Math.max(at - 1, 0)
    case 'End':
      return count - 1
    case 'Home':
      return 0
    default:
      return null
  }
}
