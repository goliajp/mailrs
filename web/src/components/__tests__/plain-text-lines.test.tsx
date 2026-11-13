import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { PlainTextLines } from '@/components/plain-text-lines'

describe('PlainTextLines', () => {
  it('gives each line of the body its own block and keeps blank lines', () => {
    const { container } = render(<PlainTextLines text={'one\n\n  three'} />)
    const lines = [...container.firstElementChild!.children]
    expect(lines).toHaveLength(3)
    expect(lines[0].textContent).toBe('one')
    expect(lines[1].querySelector('br')).not.toBeNull()
    expect(lines[2].textContent).toBe('  three')
  })

  it('renders each line through the given renderer', () => {
    const { container } = render(
      <PlainTextLines renderLine={(line) => [<b key="b">{line}</b>]} text={'a\nb'} />
    )
    expect(container.querySelectorAll('b')).toHaveLength(2)
  })
})
