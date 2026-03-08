import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Input } from '../input'

afterEach(cleanup)

describe('Input', () => {
  it('renders an input element', () => {
    render(<Input aria-label="Email" />)
    expect(screen.getByLabelText('Email').tagName).toBe('INPUT')
  })

  it('handles value changes', () => {
    const onChange = vi.fn()
    render(<Input aria-label="Name" onChange={onChange} />)
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'test' } })
    expect(onChange).toHaveBeenCalled()
  })

  it('can be disabled', () => {
    render(<Input aria-label="Disabled" disabled />)
    expect(screen.getByLabelText('Disabled').hasAttribute('disabled')).toBe(true)
  })

  it('renders placeholder', () => {
    render(<Input placeholder="Enter email..." />)
    expect(screen.getByPlaceholderText('Enter email...')).toBeDefined()
  })

  it('applies error state', () => {
    render(<Input aria-label="Error" error />)
    const input = screen.getByLabelText('Error')
    expect(input.className).toContain('danger')
  })

  it('passes through className', () => {
    render(<Input aria-label="Custom" className="my-input" />)
    expect(screen.getByLabelText('Custom').className).toContain('my-input')
  })

  it('forwards ref', () => {
    const ref = vi.fn()
    render(<Input aria-label="Ref" ref={ref} />)
    expect(ref).toHaveBeenCalled()
  })
})
