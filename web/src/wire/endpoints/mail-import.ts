import { wireFetch } from '../client'
import { mailImportReportSchema, type WireMailImportReport } from '../schemas/mail-import'

/** Upload an mbox file; the server files it message by message as it arrives. */
export function wireImportMbox(file: Blob, signal?: AbortSignal): Promise<WireMailImportReport> {
  return wireFetch(mailImportReportSchema, {
    bodyRaw: file,
    extraHeaders: { 'Content-Type': 'application/mbox' },
    method: 'POST',
    path: '/mail/import',
    signal,
  })
}
