/**
 * Files kept with a draft. A file goes into the server's upload store the
 * moment it is added; the draft then names it by `blob_id`.
 */

import type { WireDraftAttachment } from '../schemas/mail'

import { wireFetch } from '../client'
import { draftAttachmentSchema } from '../schemas/mail'

/**
 * Backend: crates/webapi/src/handlers/draft_attachments.rs — fn upload.
 * One file field; answers the `DraftAttachment` a draft save takes. The
 * server refuses a file over its per-file limit with 413 and says so.
 */
export const wireUploadDraftAttachment = (file: File): Promise<WireDraftAttachment> => {
  const fd = new FormData()
  fd.append('file', file)
  return wireFetch(draftAttachmentSchema, {
    bodyRaw: fd,
    method: 'POST',
    path: '/mail/draft-attachments',
  })
}

/**
 * Backend: crates/webapi/src/handlers/draft_attachments.rs — fn download.
 * The session cookie goes with a plain link; the name and type come back
 * as the response's disposition and content type.
 */
export function draftAttachmentUrl(a: WireDraftAttachment): string {
  const q = new URLSearchParams({ name: a.name, type: a.content_type })
  return `/api/mail/draft-attachments/${encodeURIComponent(a.blob_id)}?${q.toString()}`
}
