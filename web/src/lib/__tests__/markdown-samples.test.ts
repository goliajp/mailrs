// The send path's markdown → html_body against the shared samples, which the
// phone clients' renderers are held to byte for byte. A failure here means
// the renderer changed: regenerate with `bun scripts/markdown-samples.ts` and
// make the phones match, or put the renderer back.

import { describe, expect, it } from 'vitest'

import { wrapInEmailTemplate } from '@/components/composer/renderers/email-template'

import samples from '../markdown-samples.json'
import { renderMarkdownForSend } from '../render-markdown'

describe('markdown samples', () => {
  for (const s of samples.samples) {
    it(s.name, () => {
      expect(renderMarkdownForSend(s.markdown)).toBe(s.html)
    })
  }

  it('wraps the body in the email template', () => {
    expect(wrapInEmailTemplate(samples.wrap.inner)).toBe(samples.wrap.html)
  })

  it('keeps typed markup and unsafe links as text', () => {
    expect(renderMarkdownForSend('<script>alert(1)</script>')).not.toContain('<script')
    expect(renderMarkdownForSend('[x](javascript:alert(1))')).not.toContain('href')
  })
})
