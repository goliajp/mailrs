import type { AttachmentInfo } from '@/lib/types'

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AttachmentPreview } from '@/components/attachment-preview'

vi.mock('@/store/auth', () => ({ getToken: () => 'test-token' }))

afterEach(() => {
  cleanup()
})

function makeAttachment(overrides: Partial<AttachmentInfo> = {}): AttachmentInfo {
  return {
    content_type: 'application/pdf',
    filename: 'document.pdf',
    size: 2048,
    ...overrides,
  }
}

describe('AttachmentPreview', () => {
  it('returns null when attachments array is empty', () => {
    const { container } = render(<AttachmentPreview attachments={[]} uid={1} />)
    expect(container.innerHTML).toBe('')
  })

  it('shows attachment count', () => {
    const attachments = [
      makeAttachment({ filename: 'file1.pdf' }),
      makeAttachment({ filename: 'file2.doc' }),
    ]

    render(<AttachmentPreview attachments={attachments} uid={1} />)
    expect(screen.getByText('Attachments (2)')).toBeDefined()
  })

  it('renders file names', () => {
    const attachments = [
      makeAttachment({
        content_type: 'application/pdf',
        filename: 'report.pdf',
      }),
    ]

    render(<AttachmentPreview attachments={attachments} uid={1} />)
    expect(screen.getByText('report.pdf')).toBeDefined()
  })

  it('renders image thumbnails for image attachments', () => {
    const attachments = [
      makeAttachment({
        content_type: 'image/jpeg',
        filename: 'photo.jpg',
        size: 5120,
      }),
    ]

    const { container } = render(<AttachmentPreview attachments={attachments} uid={42} />)
    const img = container.querySelector('img')
    expect(img).not.toBeNull()
    expect(img?.getAttribute('src')).toBe('/api/mail/messages/42/attachments/0?token=test-token')
    expect(img?.getAttribute('alt')).toBe('photo.jpg')
  })

  it('renders non-image files as download links', () => {
    const attachments = [
      makeAttachment({
        content_type: 'text/csv',
        filename: 'data.csv',
        size: 1024,
      }),
    ]

    const { container } = render(<AttachmentPreview attachments={attachments} uid={5} />)
    const link = container.querySelector('a')
    expect(link).not.toBeNull()
    expect(link?.getAttribute('href')).toBe('/api/mail/messages/5/attachments/0?token=test-token')
    expect(link?.getAttribute('target')).toBe('_blank')
  })

  it('separates images from non-image files', () => {
    const attachments = [
      makeAttachment({
        content_type: 'image/png',
        filename: 'photo.png',
        size: 2048,
      }),
      makeAttachment({
        content_type: 'application/pdf',
        filename: 'doc.pdf',
        size: 4096,
      }),
    ]

    render(<AttachmentPreview attachments={attachments} uid={10} />)
    expect(screen.getByText('photo.png')).toBeDefined()
    expect(screen.getByText('doc.pdf')).toBeDefined()
  })

  it('shows formatted file size', () => {
    const attachments = [
      makeAttachment({
        content_type: 'application/zip',
        filename: 'big.zip',
        size: 1048576,
      }),
    ]

    render(<AttachmentPreview attachments={attachments} uid={1} />)
    expect(screen.getByText(/1\.0MB/)).toBeDefined()
  })

  it('detects image by file extension when content_type does not match', () => {
    const attachments = [
      makeAttachment({
        content_type: 'application/octet-stream',
        filename: 'image.webp',
      }),
    ]

    const { container } = render(<AttachmentPreview attachments={attachments} uid={1} />)
    // should render as image thumbnail (img element)
    const img = container.querySelector('img')
    expect(img).not.toBeNull()
  })

  it('opens lightbox when image thumbnail is clicked', () => {
    const attachments = [makeAttachment({ content_type: 'image/jpeg', filename: 'photo.jpg' })]

    render(<AttachmentPreview attachments={attachments} uid={1} />)

    // click the thumbnail button
    const thumbnailButton = screen.getByTitle('photo.jpg - click to enlarge')
    fireEvent.click(thumbnailButton)

    // lightbox dialog should appear
    const dialog = screen.getByRole('dialog')
    expect(dialog).toBeDefined()
    expect(screen.getByLabelText(/Image preview/)).toBeDefined()
  })

  it('closes lightbox when close button is clicked', () => {
    const attachments = [makeAttachment({ content_type: 'image/jpeg', filename: 'photo.jpg' })]

    render(<AttachmentPreview attachments={attachments} uid={1} />)

    fireEvent.click(screen.getByTitle('photo.jpg - click to enlarge'))
    expect(screen.getByRole('dialog')).toBeDefined()

    fireEvent.click(screen.getByLabelText('Close preview'))
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})
