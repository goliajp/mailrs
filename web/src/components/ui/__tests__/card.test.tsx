import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { Card } from '../card'

afterEach(cleanup)

describe('Card', () => {
  it('renders children', () => {
    render(<Card>Card content</Card>)
    expect(screen.getByText('Card content')).toBeDefined()
  })

  it('renders as a div', () => {
    render(<Card>Test</Card>)
    expect(screen.getByText('Test').tagName).toBe('DIV')
  })

  it('applies padding variant', () => {
    const { unmount } = render(<Card padding="sm">SM</Card>)
    const smClass = screen.getByText('SM').className
    unmount()

    render(<Card padding="lg">LG</Card>)
    const lgClass = screen.getByText('LG').className
    expect(smClass).not.toBe(lgClass)
  })

  it('passes through className', () => {
    render(<Card className="custom">C</Card>)
    expect(screen.getByText('C').className).toContain('custom')
  })
})
