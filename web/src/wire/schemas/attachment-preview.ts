import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/messages.rs get_attachment_content, and
// for Office files crates/webapi/src/handlers/attachment_preview.rs
export const attachmentPreviewSchema = z.object({
  extracted_text: z.string().default(''),
  sections: z.array(z.object({ lines: z.array(z.string()), title: z.string() })).default([]),
  success: z.boolean(),
  truncated: z.boolean().default(false),
})

export type WireAttachmentPreview = z.infer<typeof attachmentPreviewSchema>
