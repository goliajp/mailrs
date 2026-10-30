// A notice for new mail opens its conversation when clicked. It used to
// be shown and do nothing else.

import { afterEach, describe, expect, it, vi } from 'vitest'

import { showNewMailNotice } from '@/lib/new-mail-notice'

const shown: Array<{ close: () => void; onclick: (() => void) | null; title: string }> = []

class FakeNotification {
  static permission = 'granted'
  close = vi.fn()
  onclick: (() => void) | null = null
  title: string
  constructor(title: string) {
    this.title = title
    shown.push(this as never)
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
  shown.length = 0
})

const msg = { sender: 'Alice', snippet: 'hi', subject: 'Lunch', thread_id: 't-9' }

function hidden(value: boolean) {
  Object.defineProperty(document, 'hidden', { configurable: true, value })
}

describe('new mail notice', () => {
  it('opens the conversation it is about when clicked', () => {
    vi.stubGlobal('Notification', FakeNotification)
    hidden(true)
    const open = vi.fn()
    showNewMailNotice(msg, open)
    expect(shown.map((n) => n.title)).toEqual(['Alice'])
    shown[0]?.onclick?.()
    expect(open).toHaveBeenCalledWith('t-9')
    expect(shown[0]?.close).toHaveBeenCalled()
  })

  it('is not shown while the tab is in front', () => {
    vi.stubGlobal('Notification', FakeNotification)
    hidden(false)
    showNewMailNotice(msg, vi.fn())
    expect(shown).toEqual([])
  })
})
