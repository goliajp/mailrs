/**
 * Who signs in to the mail protocols with the account password, and the
 * per-account setting for it (`crates/webapi/src/handlers/app_passwords.rs`).
 */

import type { WirePolicy } from '../schemas/protocol-logins'

import { wireFetch } from '../client'
import { emptyResponseSchema } from '../schemas/mutations'
import { protocolLoginsSchema } from '../schemas/protocol-logins'

export function wireProtocolLogins(signal?: AbortSignal) {
  return wireFetch(protocolLoginsSchema, { path: '/admin/protocol-logins?days=30', signal })
}

export async function wireSetProtocolPassword(address: string, policy: WirePolicy): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { policy },
    method: 'PUT',
    path: `/admin/accounts/${encodeURIComponent(address)}/protocol-password`,
  })
}
