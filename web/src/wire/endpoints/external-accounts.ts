/**
 * Mailboxes somewhere else — list, add, remove, and what to fill in.
 *
 * Backend: crates/webapi/src/handlers/external_accounts.rs
 */

import { z } from 'zod'

import { wireFetch } from '../client'
import {
  externalAccountListSchema,
  externalAccountSchema,
  externalSettingsSchema,
  type WireExternalAccount,
  type WireExternalSettings,
} from '../schemas/external-accounts'

export type NewExternalAccount = {
  display_name?: string
  email: string
  incoming?: { host: string; port: number; protocol: string; tls: string }
  outgoing?: { host: string; port: number; protocol: string; tls: string }
  provider?: string
  /** The password, app password or authorisation code. Sealed on arrival. */
  secret: string
  username?: string
}

export async function wireAddExternalAccount(
  body: NewExternalAccount
): Promise<WireExternalAccount> {
  return wireFetch(externalAccountSchema, { body, method: 'POST', path: '/accounts/external' })
}

/** What to fill in for this address, before anything is saved. */
export async function wireExternalSettingsFor(email: string): Promise<WireExternalSettings> {
  return wireFetch(externalSettingsSchema, {
    path: `/accounts/external/settings?email=${encodeURIComponent(email)}`,
  })
}

export async function wireListExternalAccounts(): Promise<WireExternalAccount[]> {
  const r = await wireFetch(externalAccountListSchema, { path: '/accounts/external' })
  return r.accounts
}

export async function wireRemoveExternalAccount(id: string): Promise<void> {
  await wireFetch(z.unknown(), {
    method: 'DELETE',
    path: `/accounts/external/${encodeURIComponent(id)}`,
  })
}

/**
 * Stop or resume syncing one account.
 *
 * Pausing does not stop sending: the credential is still held and
 * still valid, and refusing to send from an address somebody owns
 * would be a second meaning nobody asked for.
 */
export async function wireSetExternalAccountPaused(
  id: string,
  paused: boolean
): Promise<WireExternalAccount> {
  return wireFetch(externalAccountSchema, {
    body: { paused },
    method: 'POST',
    path: `/accounts/external/${encodeURIComponent(id)}/paused`,
  })
}
