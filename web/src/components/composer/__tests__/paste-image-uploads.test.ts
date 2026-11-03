import { afterEach, describe, expect, it, vi } from 'vitest'

import { uploadInlineImage } from '../editor-extensions'

afterEach(() => {
  vi.unstubAllGlobals()
})

// Backend: crates/webapi/src/handlers/inline.rs — fn inline_upload answers
// `{"url": "/api/mail/inline/<id>"}` and nothing else. The schema used to
// default a missing `success` to false and the caller required it true, so
// every pasted or dropped image was uploaded and then silently not inserted.
describe('pasting an image into the composer', () => {
  it('inserts the address the server answers with', async () => {
    const url = '/api/mail/inline/0123456789abcdef0123456789abcdef'
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ url }), { status: 200 }))
    )
    const file = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], 'paste.png', {
      type: 'image/png',
    })
    expect(await uploadInlineImage(file)).toBe(url)
  })

  it('inserts nothing when the server refuses the file', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('', { status: 415 }))
    )
    const file = new File(['<svg/>'], 'x.svg', { type: 'image/svg+xml' })
    expect(await uploadInlineImage(file)).toBeNull()
  })
})
