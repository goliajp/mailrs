import type { WireImapImportStatus } from '../schemas/imap-import'

import { z } from '@/lib/zod'

import { wireFetch } from '../client'
import { imapImportStatusSchema } from '../schemas/imap-import'

/** Stop the running import after the message in hand. */
export function wireCancelImapImport(): Promise<void> {
  return wireFetch(z.undefined(), { allowEmpty: true, method: 'DELETE', path: '/mail/import/imap' })
}

/** Where the user's IMAP import stands. */
export function wireImapImportStatus(signal?: AbortSignal): Promise<WireImapImportStatus> {
  return wireFetch(imapImportStatusSchema, { path: '/mail/import/imap', signal })
}

/** Start importing from another provider's IMAP server. */
export function wireStartImapImport(body: {
  host: string
  password: string
  port: number
  username: string
}): Promise<WireImapImportStatus> {
  return wireFetch(imapImportStatusSchema, { body, method: 'POST', path: '/mail/import/imap' })
}
