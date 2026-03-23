import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { Badge } from '../badge'

afterEach(cleanup)

describe('Badge', () => {
  it('renders children', () => {
    render(<Badge>New</Badge>)
    expect(screen.getByText('New')).toBeDefined()
  })

  it('renders as span', () => {
    render(<Badge>Tag</Badge>)
    const el = screen.getByText('Tag')
    expect(el.tagName).toBe('SPAN')
  })

  it('applies intent variants', () => {
    const { rerender } = render(<Badge intent="success">OK</Badge>)
    const ok = screen.getByText('OK')
    expect(ok.className).toContain('success')

    rerender(<Badge intent="danger">Error</Badge>)
    const err = screen.getByText('Error')
    expect(err.className).toContain('danger')
  })

  it('defaults to secondary intent', () => {
    render(<Badge>Default</Badge>)
    const el = screen.getByText('Default')
    expect(el.className).toContain('secondary')
  })

  it('is non-selectable', () => {
    render(<Badge>Tag</Badge>)
    const el = screen.getByText('Tag')
    expect(el.className).toContain('select-none')
  })

  it('passes through className', () => {
    render(<Badge className="extra">X</Badge>)
    expect(screen.getByText('X').className).toContain('extra')
  })
})
