/**
 * The signed-in user's app passwords for mail apps
 * (`crates/webapi/src/handlers/app_passwords.rs`). A password is in the
 * answer that makes it, and nowhere after.
 */

import { wireFetch } from '../client'
import { appPasswordsSchema, madeAppPasswordSchema, revokedSchema } from '../schemas/app-passwords'
import { emptyResponseSchema } from '../schemas/mutations'

export function wireCreateAppPassword(name: string) {
  return wireFetch(madeAppPasswordSchema, {
    body: { name },
    method: 'POST',
    path: '/auth/app-passwords',
  })
}

export function wireListAppPasswords(signal?: AbortSignal) {
  return wireFetch(appPasswordsSchema, { path: '/auth/app-passwords', signal })
}

export function wireRevokeAllAppPasswords() {
  return wireFetch(revokedSchema, { method: 'POST', path: '/auth/app-passwords/revoke-all' })
}

export async function wireRevokeAppPassword(id: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: `/auth/app-passwords/${encodeURIComponent(id)}`,
  })
}
