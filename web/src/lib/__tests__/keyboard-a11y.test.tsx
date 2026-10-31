// Keyboard and screen-reader basics: arrows move focus within a group and
// wrap, a context menu opens with the keyboard on its first item, and new
// mail is read out through the one polite region.

import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ContextMenu } from '@/components/context-menu'
import { LiveAnnouncer } from '@/components/live-announcer'
import { announce } from '@/lib/announce'
import { rovingKeyDown } from '@/lib/roving'

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('arrow keys within a group', () => {
  it('move, wrap and jump to the ends', () => {
    render(
      <div data-testid="group" onKeyDown={(e) => rovingKeyDown(e, 'button', 'horizontal')}>
        <button type="button">one</button>
        <button type="button">two</button>
        <button type="button">three</button>
      </div>
    )
    const group = screen.getByTestId('group')
    screen.getByText('one').focus()
    fireEvent.keyDown(group, { key: 'ArrowRight' })
    expect(document.activeElement?.textContent).toBe('two')
    fireEvent.keyDown(group, { key: 'End' })
    expect(document.activeElement?.textContent).toBe('three')
    fireEvent.keyDown(group, { key: 'ArrowRight' })
    expect(document.activeElement?.textContent).toBe('one')
    fireEvent.keyDown(group, { key: 'ArrowLeft' })
    expect(document.activeElement?.textContent).toBe('three')
    fireEvent.keyDown(group, { key: 'Home' })
    expect(document.activeElement?.textContent).toBe('one')
    // the other axis is left alone
    fireEvent.keyDown(group, { key: 'ArrowDown' })
    expect(document.activeElement?.textContent).toBe('one')
  })
})

describe('context menu', () => {
  it('opens on its first item and moves with the arrows', () => {
    const items = ['Archive', 'Star', 'Delete'].map((label) => ({ label, onClick: vi.fn() }))
    render(<ContextMenu items={items} onClose={vi.fn()} position={{ x: 10, y: 10 }} />)
    expect(document.activeElement?.textContent).toBe('Archive')
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowDown' })
    expect(document.activeElement?.textContent).toBe('Star')
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowUp' })
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowUp' })
    expect(document.activeElement?.textContent).toBe('Delete')
  })
})

describe('live announcer', () => {
  it('reads out what it is given, and a repeat again', async () => {
    vi.useFakeTimers({ toFake: ['requestAnimationFrame'] })
    render(<LiveAnnouncer />)
    const region = screen.getByTestId('live-announcer')
    expect(region.getAttribute('aria-live')).toBe('polite')
    act(() => announce('New mail from Ann: lunch'))
    act(() => vi.advanceTimersToNextFrame())
    expect(region.textContent).toBe('New mail from Ann: lunch')
    act(() => announce('New mail from Ann: lunch'))
    expect(region.textContent).toBe('')
    act(() => vi.advanceTimersToNextFrame())
    expect(region.textContent).toBe('New mail from Ann: lunch')
  })
})
