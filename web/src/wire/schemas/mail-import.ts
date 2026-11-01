import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/mbox/import.rs Report
export const mailImportReportSchema = z.object({
  archived: z.number(),
  complete: z.boolean(),
  failed: z.number(),
  imported: z.number(),
  sent: z.number(),
  skipped_duplicate: z.number(),
  skipped_junk_or_trash: z.number(),
  too_large: z.number(),
})

export type WireMailImportReport = z.infer<typeof mailImportReportSchema>
