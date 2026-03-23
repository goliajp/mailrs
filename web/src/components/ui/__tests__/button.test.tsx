import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Button } from '../button'

afterEach(cleanup)

describe('Button', () => {
  it('renders children', () => {
    render(<Button>Click me</Button>)
    expect(screen.getByText('Click me')).toBeDefined()
  })

  it('renders as a button element by default', () => {
    render(<Button>Test</Button>)
    expect(screen.getByRole('button')).toBeDefined()
  })

  it('handles click', () => {
    const onClick = vi.fn()
    render(<Button onClick={onClick}>Click</Button>)
    fireEvent.click(screen.getByRole('button'))
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('can be disabled', () => {
    const onClick = vi.fn()
    render(
      <Button disabled onClick={onClick}>
        Disabled
      </Button>
    )
    const btn = screen.getByRole('button')
    expect(btn.hasAttribute('disabled')).toBe(true)
    fireEvent.click(btn)
    expect(onClick).not.toHaveBeenCalled()
  })

  it('applies variant styles', () => {
    const { unmount } = render(<Button variant="primary">Primary</Button>)
    const primaryClass = screen.getByRole('button').className
    expect(primaryClass).toContain('bg-')
    unmount()

    render(<Button variant="ghost">Ghost</Button>)
    const ghostClass = screen.getByRole('button').className
    expect(ghostClass).not.toBe(primaryClass)
  })

  it('applies size styles', () => {
    const { unmount } = render(<Button size="sm">Small</Button>)
    const smClass = screen.getByRole('button').className
    unmount()

    render(<Button size="lg">Large</Button>)
    const lgClass = screen.getByRole('button').className
    expect(smClass).not.toBe(lgClass)
  })

  it('passes through className', () => {
    render(<Button className="custom-class">Custom</Button>)
    expect(screen.getByRole('button').className).toContain('custom-class')
  })

  it('forwards ref', () => {
    const ref = vi.fn()
    render(<Button ref={ref}>Ref</Button>)
    expect(ref).toHaveBeenCalled()
  })

  it('accepts type attribute', () => {
    render(<Button type="submit">Submit</Button>)
    expect(screen.getByRole('button').getAttribute('type')).toBe('submit')
  })
})
