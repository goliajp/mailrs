import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { IconButton } from '../icon-button'

afterEach(cleanup)

describe('IconButton', () => {
  it('renders with aria-label', () => {
    render(
      <IconButton label="Close">
        <span>X</span>
      </IconButton>
    )
    expect(screen.getByLabelText('Close')).toBeDefined()
  })

  it('handles click', () => {
    const onClick = vi.fn()
    render(
      <IconButton label="Action" onClick={onClick}>
        <span>+</span>
      </IconButton>
    )
    fireEvent.click(screen.getByLabelText('Action'))
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('can be disabled', () => {
    render(
      <IconButton disabled label="Disabled">
        <span>X</span>
      </IconButton>
    )
    expect(screen.getByLabelText('Disabled').hasAttribute('disabled')).toBe(
      true
    )
  })

  it('applies size variant', () => {
    const { unmount } = render(
      <IconButton label="Small" size="sm">
        <span>S</span>
      </IconButton>
    )
    const smClass = screen.getByLabelText('Small').className
    unmount()

    render(
      <IconButton label="Large" size="lg">
        <span>L</span>
      </IconButton>
    )
    const lgClass = screen.getByLabelText('Large').className
    expect(smClass).not.toBe(lgClass)
  })

  it('supports title attribute', () => {
    render(
      <IconButton label="Star" title="Star">
        <span>*</span>
      </IconButton>
    )
    expect(screen.getByTitle('Star')).toBeDefined()
  })

  it('passes through className', () => {
    render(
      <IconButton className="my-class" label="Custom">
        <span>C</span>
      </IconButton>
    )
    expect(screen.getByLabelText('Custom').className).toContain('my-class')
  })
})
