import type { AttachmentInfo } from '@/lib/types'

/**
 * Which attachments are the reader's, and which are the sender's
 * decoration.
 *
 * A `multipart/related` message carries its inline images as parts with
 * a `Content-ID`, and the HTML points at them with `src="cid:…"`. Those
 * parts are already on screen — `rewriteCidImages` draws them where the
 * sender put them — so listing them again under "Attachments" describes
 * a paperclip that does not exist. The message that prompted this was an
 * Outlook forward whose only "attachment" was a 32×32 folder icon from
 * the signature block: valid, served, and meaningless.
 *
 * Measured over 4,000 messages in the production mailbox: 145 carry
 * attachments, 9 carry a cid-referenced inline image, and for **6** of
 * those the inline image is the *whole* attachment row — those six rows
 * are pure noise today. Three carry both, and there the row shrinks to
 * the genuine file rather than disappearing.
 *
 * The rule is deliberately narrow: hide a part only when it has a
 * Content-ID **and** the HTML we are about to render actually references
 * it. An inline part nobody references stays listed — it is unreachable
 * otherwise, and a file the reader cannot get to is the worse failure.
 */

/** Every `cid:` target in `html`, lowercased and unbracketed. */
export function referencedCids(html: string): Set<string> {
  const found = new Set<string>()
  if (!html.includes('cid:')) return found
  // Stops at a quote, whitespace or `>` — the delimiters an `src`
  // attribute can end on. Unquoted attributes are why whitespace counts.
  for (const m of html.matchAll(/cid:([^"'\s>]+)/gi)) {
    const cid = normalizeCid(m[1])
    if (cid) found.add(cid)
  }
  return found
}

/**
 * The attachments worth showing, each with the index the download URL
 * needs.
 *
 * The index is the position in the **original** array: it is what
 * `/api/mail/messages/{uid}/attachments/{index}` resolves against, and
 * renumbering after a filter would hand out the wrong file.
 */
export function visibleAttachments(
  attachments: AttachmentInfo[],
  html: null | string | undefined
): { att: AttachmentInfo; index: number }[] {
  const cids = referencedCids(html ?? '')
  return attachments
    .map((att, index) => ({ att, index }))
    .filter(({ att }) => {
      if (cids.size === 0) return true
      const cid = normalizeCid(att.content_id ?? '')
      if (!cid) return true
      return !cids.has(cid)
    })
}

/** `<abc@d.com>` and `ABC@D.com` are the same content-id. */
function normalizeCid(raw: string): string {
  return raw.replace(/^<|>$/g, '').trim().toLowerCase()
}
