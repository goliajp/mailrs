import { z } from '@/lib/zod'

import { mailImportReportSchema } from './mail-import'

// Backend: crates/webapi/src/handlers/imap_import/job.rs — struct Status
export const imapImportStatusSchema = z.object({
  error: z.string().nullable(),
  folder: z.string().nullable(),
  host: z.string(),
  port: z.number(),
  report: mailImportReportSchema,
  state: z.enum(['queued', 'running', 'done', 'failed', 'cancelled', 'interrupted', 'none']),
  username: z.string(),
})

export type WireImapImportStatus = z.infer<typeof imapImportStatusSchema>
