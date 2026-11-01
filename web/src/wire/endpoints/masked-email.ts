/**
 * The account's masked addresses (`crates/webapi/src/handlers/masked_email.rs`).
 */

import type { WireMaskedEmail } from '../schemas/masked-email'

import { wireFetch } from '../client'
import { maskedEmailListSchema, maskedEmailSchema } from '../schemas/masked-email'
import { emptyResponseSchema } from '../schemas/mutations'

export type MaskedEmailState = 'disabled' | 'enabled'

export function wireCreateMaskedEmail(body: {
  description: string
  forDomain: string
}): Promise<WireMaskedEmail> {
  return wireFetch(maskedEmailSchema, { body, method: 'POST', path: '/masked-emails' })
}

export async function wireDeleteMaskedEmail(email: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/masked-emails/${encodeURIComponent(email)}`,
  })
}

export function wireListMaskedEmails(signal?: AbortSignal): Promise<WireMaskedEmail[]> {
  return wireFetch(maskedEmailListSchema, { path: '/masked-emails', signal })
}

export function wireSetMaskedEmailState(
  email: string,
  state: MaskedEmailState
): Promise<WireMaskedEmail> {
  return wireFetch(maskedEmailSchema, {
    body: { state },
    method: 'PATCH',
    path: `/masked-emails/${encodeURIComponent(email)}`,
  })
}
