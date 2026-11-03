import { render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TextBlock } from '@/components/composer/blocks/text-block'

// how often the box asked the page for its laid-out height
let measured = 0

beforeEach(() => {
  measured = 0
  Object.defineProperty(HTMLTextAreaElement.prototype, 'scrollHeight', {
    configurable: true,
    get: () => {
      measured += 1
      return 400
    },
  })
})
afterEach(() => {
  delete (HTMLTextAreaElement.prototype as { scrollHeight?: number }).scrollHeight
  vi.restoreAllMocks()
})

function box(content: string) {
  const { container } = render(
    <TextBlock
      data={{ content, format: 'markdown', html: '' }}
      onChange={() => {}}
      onSubmit={() => {}}
    />
  )
  return container.querySelector('textarea') as HTMLTextAreaElement
}

describe('the reply box sizes itself', () => {
  // the box mounts empty with every conversation opened; measuring it
  // there forced a layout of the whole page inside that commit
  it('empty, at its minimum height without measuring anything', () => {
    const el = box('')
    expect(measured).toBe(0)
    expect(el.style.height).toBe('240px')
  })

  it('with text, to fit the text', () => {
    const el = box('a line\nanother')
    expect(measured).toBeGreaterThan(0)
    expect(el.style.height).toBe('400px')
  })
})
