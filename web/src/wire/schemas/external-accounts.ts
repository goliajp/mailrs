import { z } from 'zod'

/**
 * Mailboxes somewhere else.
 *
 * Backend: crates/webapi/src/handlers/external_accounts.rs
 *   - `list`         GET  /api/accounts/external      -> { accounts: AccountRow[] }
 *   - `create`       POST /api/accounts/external      -> AccountRow
 *   - `delete`       DELETE /api/accounts/external/{id} -> 204
 *   - `settings_for` GET  /api/accounts/external/settings?email= -> known|autodiscover
 *
 * The row shape is `mailrs_core_sidestate::families::external_accounts::AccountRow`
 * (crates/core-sidestate/src/families/external_accounts.rs). Verified against
 * both files 2026-08-23.
 *
 * **There is no secret field, by construction.** The Rust side asserts the
 * serialised row carries none; this schema would happily pass one through, so
 * the guarantee lives there and this comment points at it.
 */

export const externalEndpointSchema = z.object({
  host: z.string(),
  port: z.number(),
  protocol: z.string(),
  tls: z.enum(['implicit', 'starttls', 'none']),
})

export const externalAccountSchema = z.object({
  auth: z.enum(['password', 'app_password', 'oauth2']),
  colour: z.string().nullish(),
  created_at: z.number().default(0),
  display_name: z.string(),
  email: z.string(),
  failures: z.number().default(0),
  id: z.string(),
  incoming: externalEndpointSchema,
  last_error: z.string().nullish(),
  last_sync: z.number().default(0),
  next_attempt: z.number().default(0),
  outgoing: externalEndpointSchema,
  provider: z.string(),
  sort: z.number().default(0),
  // A row written before a state existed reads as working, which is the
  // same default the Rust side takes.
  state: z.enum(['ok', 'needs_auth', 'error', 'paused']).default('ok'),
  username: z.string().nullish(),
})

export const externalAccountListSchema = z.object({
  accounts: z.array(externalAccountSchema).default([]),
})

const secretHelpSchema = z.object({ url: z.string(), what: z.string() })

/** What a set-up screen should fill in, before anything is saved. */
export const externalSettingsSchema = z.union([
  z.object({
    known: z.literal(true),
    preset: z.object({
      auth: z.enum(['password', 'app_password', 'oauth2']),
      id: z.string(),
      imap: externalEndpointSchema,
      label: z.string(),
      secret_help: secretHelpSchema.nullish(),
      skip_folders: z.array(z.string()).default([]),
      smtp: externalEndpointSchema,
    }),
  }),
  z.object({
    autodiscover: z.array(z.record(z.string(), z.unknown())).default([]),
    known: z.literal(false),
  }),
])

export type WireExternalAccount = z.infer<typeof externalAccountSchema>
export type WireExternalSettings = z.infer<typeof externalSettingsSchema>
