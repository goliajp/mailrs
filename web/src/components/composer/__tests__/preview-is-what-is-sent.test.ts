import { describe, expect, it } from 'vitest'

import { assembleEmail } from '@/components/composer/assembly-engine'
import { previewHtml } from '@/components/composer/preview'
import { createBlock } from '@/components/composer/types'

/**
 * The preview and the send path must be the same bytes.
 *
 * They were not. "Preview" was a tab inside the *text block*: it
 * rendered that one block's markdown through the app's prose classes,
 * on the app's dark background. What actually went out was
 * `assembleEmail(blocks).html` — a whole HTML document with a
 * system-font `<body>`, a 600px wrapper, and the signature and quoted
 * history appended after the text. So the sender approved a fragment
 * in the app's skin and the recipient received a different document,
 * which is why "sent 以后和 preview 完全不一样".
 *
 * These tests pin the property rather than the markup: whatever the
 * assembler produces is what the preview shows. A future block type,
 * wrapper change or style tweak cannot make them drift apart without
 * failing here.
 */

function textBlock(content: string) {
  return createBlock('text', { content, format: 'markdown', html: '' })
}

describe('preview is what is sent', () => {
  it('previews the exact document the send path puts on the wire', () => {
    const blocks = [textBlock('Hello **world**')]
    expect(previewHtml(blocks)).toBe(assembleEmail(blocks).html)
  })

  it('includes the signature, which the block preview never showed', () => {
    const blocks = [
      textBlock('Morning'),
      createBlock('signature', { html: '<p>— Lihao</p>', text: '— Lihao' }),
    ]
    const preview = previewHtml(blocks)
    expect(preview).toContain('Lihao')
    expect(preview).toBe(assembleEmail(blocks).html)
  })

  it('includes the quoted history, which the block preview never showed', () => {
    const blocks = [
      textBlock('Agreed.'),
      createBlock('quote', {
        collapsed: false,
        headerHtml: '',
        headerText: '',
        html: '<blockquote>the earlier letter</blockquote>',
      }),
    ]
    const preview = previewHtml(blocks)
    expect(preview).toContain('the earlier letter')
    expect(preview).toBe(assembleEmail(blocks).html)
  })

  /**
   * The recipient's client supplies no stylesheet of ours, so the mail
   * carries its own. A preview that borrows the app's prose classes is
   * showing a document that will never exist.
   */
  it('carries the mail its own styles rather than the app skin', () => {
    const html = previewHtml([textBlock('hi')])
    expect(html).toContain('<body')
    expect(html).toContain('max-width:600px')
  })

  it('is empty for an empty composer rather than a bare shell', () => {
    expect(previewHtml([])).toBe('')
    expect(previewHtml([textBlock('   ')])).toBe('')
  })
})
