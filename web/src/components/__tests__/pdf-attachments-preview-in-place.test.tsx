// A PDF attachment in the desktop reading pane can be previewed in place.
// The only preview was in the mobile bubble, behind a text-extraction
// toggle, and in a sandboxed frame Chrome shows no PDF in.

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { AttachmentPreview } from '@/components/attachment-preview'

afterEach(cleanup)

const pdf = { content_type: 'application/pdf', filename: 'invoice.pdf', size: 2048 }
const renamed = { content_type: 'text/html', filename: 'not-really.pdf', size: 10 }

describe('pdf attachments', () => {
  it('preview in place, unsandboxed, at the attachment url', () => {
    render(<AttachmentPreview attachments={[pdf] as never} uid={7} />)
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const frame = screen.getByTitle('invoice.pdf')
    expect(frame.tagName).toBe('IFRAME')
    expect(frame.getAttribute('src')).toBe('/api/mail/messages/7/attachments/0')
    expect(frame.hasAttribute('sandbox')).toBe(false)
    fireEvent.click(screen.getByRole('button', { name: 'Hide preview' }))
    expect(screen.queryByTitle('invoice.pdf')).toBeNull()
  })

  it('are not offered for a part only named like one', () => {
    render(<AttachmentPreview attachments={[renamed] as never} uid={7} />)
    expect(screen.queryByRole('button', { name: 'Preview' })).toBeNull()
  })
})
