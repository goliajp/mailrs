import type { AnyBlock, AssembledEmail } from './types'

import { wrapInEmailTemplate } from './renderers/email-template'
import { renderBlockHtml, renderBlockText } from './renderers/html-renderer'

export function assembleEmail(blocks: ReadonlyArray<AnyBlock>): AssembledEmail {
  const htmlParts: string[] = []
  const textParts: string[] = []
  const attachments: File[] = []

  for (const block of blocks) {
    const html = renderBlockHtml(block)
    const text = renderBlockText(block)

    if (html) htmlParts.push(html)
    if (text) textParts.push(text)

    if (block.type === 'attachment') {
      const d = block.data as { file: File }
      attachments.push(d.file)
    }
  }

  return {
    attachments,
    html: wrapInEmailTemplate(htmlParts.join('\n')),
    text: textParts.join('\n\n'),
  }
}
