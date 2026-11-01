import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { OfficePreview } from '../office-preview'

const preview = vi.hoisted(() => vi.fn())
vi.mock('@/wire/endpoints/attachment-preview', () => ({ wireAttachmentPreview: preview }))

afterEach(() => {
  cleanup()
  preview.mockReset()
})

function open() {
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <OfficePreview index={1} uid={7} />
    </QueryClientProvider>
  )
  fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
}

describe('OfficePreview', () => {
  it('shows a document as its paragraphs and a sheet as a table', async () => {
    preview.mockResolvedValue({
      extracted_text: '',
      sections: [
        { lines: ['Quarterly report'], title: '' },
        { lines: ['Item\tPaid', 'Rent\t950'], title: 'Q1' },
      ],
      success: true,
      truncated: true,
    })
    open()
    expect(await screen.findByText('Quarterly report')).toBeDefined()
    expect(screen.getByText('Q1')).toBeDefined()
    expect(screen.getByRole('table')).toBeDefined()
    expect(screen.getByText('950').tagName).toBe('TD')
    expect(screen.getByText(/Only the beginning is shown/)).toBeDefined()
    expect(preview).toHaveBeenCalledWith(7, 1, expect.anything())
  })

  it('says so when the file could not be read', async () => {
    preview.mockResolvedValue({
      extracted_text: '',
      sections: [],
      success: false,
      truncated: false,
    })
    open()
    expect(await screen.findByText(/could not be read here/)).toBeDefined()
    expect(screen.queryByRole('table')).toBeNull()
  })
})
