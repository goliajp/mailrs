import type { AnyBlock, AssembledEmail } from './types'

import { wrapInEmailTemplate } from './renderers/email-template'
import { renderBlockHtml, renderBlockText } from './renderers/html-renderer'

export function assembleEmail(blocks: ReadonlyArray<AnyBlock>): AssembledEmail {
  const htmlParts: string[] = []
  const textParts: string[] = []

  for (const block of blocks) {
    const html = renderBlockHtml(block)
    const text = renderBlockText(block)

    if (html) htmlParts.push(html)
    if (text) textParts.push(text)
  }

  return {
    html: wrapInEmailTemplate(htmlParts.join('\n')),
    text: textParts.join('\n\n'),
  }
}
