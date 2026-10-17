import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { CodeChips, LinkList } from '../extracted'

const writeText = vi.fn(() => Promise.resolve())

beforeEach(() => {
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
})

afterEach(() => {
  writeText.mockClear()
})

describe('CodeChips', () => {
  it('shows every code and copies the one clicked', async () => {
    render(<CodeChips codes={['482913', 'AB12CD']} />)
    const list = screen.getByRole('list', { name: 'Codes' })
    expect(within(list).getAllByRole('listitem')).toHaveLength(2)
    expect(within(list).getByText('AB12CD')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Copy code AB12CD' }))
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('AB12CD'))
  })

  it('says so when there are none', () => {
    render(<CodeChips codes={[]} />)
    expect(screen.getByText('No codes found')).toBeInTheDocument()
    expect(screen.queryByRole('list', { name: 'Codes' })).toBeNull()
  })
})

describe('LinkList', () => {
  it('lists each link with its text and copies the url', async () => {
    const url = 'https://example.com/verify?token=abc'
    render(
      <LinkList
        links={[
          { text: 'Verify', url },
          { text: '', url: 'https://example.com/help' },
        ]}
      />
    )
    expect(screen.getByText('Verify')).toBeInTheDocument()
    const anchor = screen.getByRole('link', { name: url })
    expect(anchor).toHaveAttribute('href', url)
    expect(anchor).toHaveAttribute('rel', 'noopener noreferrer')

    fireEvent.click(screen.getAllByRole('button', { name: 'Copy link' })[0])
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(url))
  })
})
