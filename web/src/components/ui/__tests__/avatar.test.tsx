import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { Avatar } from '../avatar'

afterEach(cleanup)

describe('Avatar', () => {
  it('renders initial character', () => {
    render(<Avatar name="Alice" />)
    expect(screen.getByText('A')).toBeDefined()
  })

  it('renders first character of email when no display name', () => {
    render(<Avatar name="bob@example.com" />)
    expect(screen.getByText('B')).toBeDefined()
  })

  it('applies size variant', () => {
    const { unmount } = render(<Avatar name="A" size="sm" />)
    const smClass = screen.getByText('A').className
    unmount()

    render(<Avatar name="A" size="lg" />)
    const lgClass = screen.getByText('A').className
    expect(smClass).not.toBe(lgClass)
  })

  it('renders as a div', () => {
    render(<Avatar name="Test" />)
    const el = screen.getByText('T')
    expect(el.tagName).toBe('DIV')
  })

  it('generates consistent color for same name', () => {
    const { unmount } = render(<Avatar name="Alice" />)
    const class1 = screen.getByText('A').className
    unmount()

    render(<Avatar name="Alice" />)
    const class2 = screen.getByText('A').className
    expect(class1).toBe(class2)
  })
})
