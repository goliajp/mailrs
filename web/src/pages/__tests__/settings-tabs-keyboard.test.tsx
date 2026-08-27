import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

// The section list is a roving-tabindex tablist: eight of nine tabs
// carry `tabIndex={-1}`, which is only correct if arrow keys move the
// roving point. There were none, so a keyboard user reached the active
// tab and could not get to the other eight — nine settings screens
// behind a wall.

vi.mock('@/components/settings/general-section', () => ({
  GeneralSection: () => <div>general</div>,
}))

const { Settings } = await import('@/pages/settings')

afterEach(cleanup)

function renderSettings() {
  return render(
    <MemoryRouter>
      <Settings />
    </MemoryRouter>
  )
}

describe('the settings section list', () => {
  it('moves with the arrow keys', () => {
    renderSettings()
    const tabs = screen.getAllByRole('tab')
    expect(tabs.length).toBeGreaterThan(2)
    // The first tab is the roving point at rest.
    expect(tabs[0].getAttribute('tabindex')).toBe('0')

    fireEvent.keyDown(tabs[0].parentElement as HTMLElement, { key: 'ArrowDown' })
    const after = screen.getAllByRole('tab')
    expect(after[1].getAttribute('tabindex'), 'ArrowDown did not move the roving point').toBe('0')
    expect(after[0].getAttribute('tabindex')).toBe('-1')
  })

  it('wraps, and Home and End reach the ends', () => {
    renderSettings()
    const list = screen.getAllByRole('tab')[0].parentElement as HTMLElement

    fireEvent.keyDown(list, { key: 'ArrowUp' })
    let tabs = screen.getAllByRole('tab')
    expect(tabs[tabs.length - 1].getAttribute('tabindex'), 'ArrowUp did not wrap').toBe('0')

    fireEvent.keyDown(list, { key: 'Home' })
    tabs = screen.getAllByRole('tab')
    expect(tabs[0].getAttribute('tabindex')).toBe('0')

    fireEvent.keyDown(list, { key: 'End' })
    tabs = screen.getAllByRole('tab')
    expect(tabs[tabs.length - 1].getAttribute('tabindex')).toBe('0')
  })

  it('leaves other keys alone', () => {
    renderSettings()
    const list = screen.getAllByRole('tab')[0].parentElement as HTMLElement
    fireEvent.keyDown(list, { key: 'a' })
    expect(screen.getAllByRole('tab')[0].getAttribute('tabindex')).toBe('0')
  })
})
