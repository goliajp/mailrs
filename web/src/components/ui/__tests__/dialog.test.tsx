import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Dialog } from '../dialog'

afterEach(cleanup)

describe('Dialog', () => {
  it('renders children when open', () => {
    render(<Dialog open onClose={vi.fn()}>Content</Dialog>)
    expect(screen.getByText('Content')).toBeDefined()
  })

  it('does not render when closed', () => {
    render(<Dialog open={false} onClose={vi.fn()}>Hidden</Dialog>)
    expect(screen.queryByText('Hidden')).toBeNull()
  })

  it('has dialog role and aria-modal', () => {
    render(<Dialog open onClose={vi.fn()}>Test</Dialog>)
    const dialog = screen.getByRole('dialog')
    expect(dialog.getAttribute('aria-modal')).toBe('true')
  })

  it('calls onClose when backdrop is clicked', () => {
    const onClose = vi.fn()
    render(<Dialog open onClose={onClose}>Test</Dialog>)
    // click the backdrop (the outer overlay div)
    const backdrop = screen.getByRole('dialog').parentElement!
    fireEvent.click(backdrop)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('does not call onClose when content is clicked', () => {
    const onClose = vi.fn()
    render(<Dialog open onClose={onClose}><p>Inner</p></Dialog>)
    fireEvent.click(screen.getByText('Inner'))
    expect(onClose).not.toHaveBeenCalled()
  })

  it('calls onClose on Escape key', () => {
    const onClose = vi.fn()
    render(<Dialog open onClose={onClose}>Test</Dialog>)
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('accepts title prop', () => {
    render(<Dialog open onClose={vi.fn()} title="Confirm">Body</Dialog>)
    expect(screen.getByText('Confirm')).toBeDefined()
  })
})
