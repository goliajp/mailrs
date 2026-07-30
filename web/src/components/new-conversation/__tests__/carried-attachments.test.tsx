import type { ComposeRedraftSource } from '@/store/ui'

import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { CarriedAttachments } from '../carried-attachments'
import { carriedSelection } from '../carried-model'

function source(count: number): ComposeRedraftSource {
  return {
    attachments: Array.from({ length: count }, (_, i) => ({
      content_type: 'image/png',
      // Same filename on purpose: it is why the wire carries indices.
      filename: 'image.png',
      index: i,
      size: 1024 * (i + 1),
    })),
    bcc: '',
    body: 'b',
    cc: '',
    inReplyTo: null,
    redraftOf: 'orig@golia.jp',
    subject: 's',
    to: 'a@x.com',
  }
}

afterEach(cleanup)

describe('carriedSelection', () => {
  /// The distinction the whole design rests on. `null` keeps everything,
  /// `[]` keeps nothing; collapsing them re-attaches files the user
  /// deleted, and they find out after sending.
  it('tells keeping nothing apart from having nothing to keep', () => {
    const s = source(2)
    expect(carriedSelection(s, new Set([0, 1]))).toEqual([0, 1])
    expect(carriedSelection(s, new Set())).toEqual([])
    // Nothing to select from at all — not an empty selection.
    expect(carriedSelection(source(0), new Set())).toBeNull()
    expect(carriedSelection(null, new Set())).toBeNull()
  })

  it('keeps only what is still selected, by index', () => {
    expect(carriedSelection(source(3), new Set([0, 2]))).toEqual([0, 2])
  })

  /// A stale index in the kept set must not invent an attachment. The list
  /// is driven by what the envelope has, not by what the set holds.
  it('ignores an index the send does not carry', () => {
    expect(carriedSelection(source(2), new Set([0, 9]))).toEqual([0])
  })
})

describe('CarriedAttachments', () => {
  it('renders nothing when the send carries no files', () => {
    const { container } = render(
      <CarriedAttachments items={[]} kept={new Set()} onToggle={vi.fn()} />
    )
    expect(container.firstChild).toBeNull()
  })

  /// Two parts share one filename, so the remove control has to be
  /// distinguishable per row — by index, since the name cannot do it.
  it('gives each identically-named file its own control', () => {
    render(
      <CarriedAttachments items={source(2).attachments} kept={new Set([0, 1])} onToggle={vi.fn()} />
    )
    expect(screen.getAllByRole('button', { name: 'Remove image.png' })).toHaveLength(2)
  })

  /// A dropped file stays on screen so removing the wrong one is
  /// recoverable without reopening the whole re-edit.
  it('keeps a dropped file visible, offering to keep it again', () => {
    render(<CarriedAttachments items={source(1).attachments} kept={new Set()} onToggle={vi.fn()} />)
    expect(screen.getByRole('button', { name: 'Keep image.png' })).toBeTruthy()
  })

  it('reports the index that was clicked', () => {
    const onToggle = vi.fn()
    render(
      <CarriedAttachments
        items={source(3).attachments}
        kept={new Set([0, 1, 2])}
        onToggle={onToggle}
      />
    )
    screen.getAllByRole('button', { name: 'Remove image.png' })[2].click()
    expect(onToggle).toHaveBeenCalledWith(2)
  })
})
