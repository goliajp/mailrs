import type { AnyBlock } from './types'

import { assembleEmail } from './assembly-engine'

/**
 * The document the recipient will get, for showing to the sender
 * before it goes.
 *
 * There is deliberately no rendering of its own here: it calls the
 * assembler the send path calls, so the preview cannot describe a mail
 * that will not exist. The previous preview lived inside the text
 * block and rendered only that block's markdown through the app's
 * prose classes — no `<body>`, no 600px wrapper, no signature, no
 * quoted history, and the app's dark background behind it. Everything
 * a sender checked in Preview was true of a document nobody would
 * receive.
 *
 * Empty when there is nothing to send: `assembleEmail` still returns
 * the wrapper for an empty block list, and showing that shell would
 * report a blank page as if it were the mail.
 */
export function previewHtml(blocks: ReadonlyArray<AnyBlock>): string {
  const assembled = assembleEmail(blocks)
  if (!assembled.text.trim() && !hasVisibleHtml(assembled.html)) return ''
  return assembled.html
}

/** Whether anything survives once the wrapper's own markup is gone. */
function hasVisibleHtml(html: string): boolean {
  const body = html.slice(html.indexOf('<body'))
  return (
    body
      .replace(/<[^>]*>/g, '')
      .replace(/&nbsp;/g, ' ')
      .trim().length > 0
  )
}
