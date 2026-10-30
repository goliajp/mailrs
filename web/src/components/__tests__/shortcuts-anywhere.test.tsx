// `?` shows the shortcuts on every page; it was handled by the mail page
// alone. Typing a question mark into a field still types it.

import { act, cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it } from 'vitest'

import { ShortcutsHost } from '@/components/keyboard-shortcuts-dialog'

afterEach(cleanup)

function press(target: EventTarget) {
  act(() => {
    target.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: '?' }))
  })
}

describe('shortcuts sheet', () => {
  it('opens with ? outside the mail page, but not from a text field', () => {
    render(
      <Provider store={createStore()}>
        <input aria-label="note" />
        <ShortcutsHost />
      </Provider>
    )
    press(screen.getByLabelText('note'))
    expect(screen.queryByText('Keyboard Shortcuts')).toBeNull()
    press(window)
    expect(screen.getByText('Keyboard Shortcuts')).toBeDefined()
    expect(screen.getByText('Command palette')).toBeDefined()
  })
})
