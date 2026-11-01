import { wireFetch } from '../client'
import { attachmentPreviewSchema, type WireAttachmentPreview } from '../schemas/attachment-preview'

/** What an attachment says, read on the server: Office files by section. */
export function wireAttachmentPreview(
  uid: number,
  index: number,
  signal?: AbortSignal
): Promise<WireAttachmentPreview> {
  return wireFetch(attachmentPreviewSchema, {
    path: `/mail/messages/${uid}/attachments/${index}/content`,
    signal,
  })
}
