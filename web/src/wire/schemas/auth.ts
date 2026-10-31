/**
 * Auth wire schemas — v2.1 §8 batch 2 (2026-07-08).
 *
 * OIDC config, login (with TOTP-required union branch), /auth/me
 * profile refresh, TOTP setup/status, recovery email get/set.
 */

import { z } from '@/lib/zod'

import { emptyResponseSchema } from './mutations'

// ── /auth/oidc/config — public discovery ─────────────────────────

/**
 * v2.1 §8 (2026-07-08): backend returns `{enabled, providers[]}`
 * per `crates/webapi/src/handlers/complete.rs::oidc_config`. Frontend
 * login.tsx historically read a flat `{enabled, login_url,
 * provider_name}` shape — schema accepts both to keep migration
 * pain-free while the UI type reconciles as follow-up.
 */
export const oidcConfigSchema = z.object({
  enabled: z.boolean().default(false),
  login_url: z.string().optional(),
  provider_name: z.string().optional(),
  providers: z.array(z.unknown()).optional(),
})

export type WireOidcConfig = z.infer<typeof oidcConfigSchema>

// ── /auth/me — profile refresh ────────────────────────────────────

export const authMeSchema = z.object({
  accessible_domains: z.array(z.string()).default([]),
  address: z.string(),
  display_name: z.string().default(''),
  is_admin: z.boolean().default(false),
  is_reset_issuer: z.boolean().default(false),
  is_super: z.boolean().default(false),
  permissions: z.array(z.string()).default([]),
})

export type WireAuthMe = z.infer<typeof authMeSchema>

// ── password cleared / reset codes ──────────────────────────────
// Backend: crates/webapi/src/handlers/auth/session.rs — fn password_cleared_response
export const passwordClearedSchema = z.object({
  contact: z.object({ address: z.string(), name: z.string() }),
  error: z.literal('password_cleared'),
})

// Backend: crates/webapi/src/handlers/reset_code.rs — fn issue
export const resetCodeIssuedSchema = z.object({
  code: z.string(),
  expires_at: z.number(),
})

export type WireResetCodeIssued = z.infer<typeof resetCodeIssuedSchema>

// Backend: crates/webapi/src/handlers/reset_code.rs — fn status
export const resetCodeStatusSchema = z.union([
  z.object({ issued: z.literal(false) }),
  z.object({
    expires_at: z.number().nullable(),
    failures: z.number(),
    issued: z.literal(true),
    issued_at: z.number().nullable(),
    issued_by: z.string(),
    key_current: z.boolean(),
  }),
])

export type WireResetCodeStatus = z.infer<typeof resetCodeStatusSchema>

// ── /auth/login — union: TOTP-required OR auth token ─────────────

const loginTotpRequiredSchema = z.object({
  requires_totp: z.literal(true),
})

const loginSuccessSchema = z.object({
  accessible_domains: z.array(z.string()).default([]),
  address: z.string(),
  display_name: z.string().default(''),
  is_admin: z.boolean().default(false),
  is_super: z.boolean().default(false),
  permissions: z.array(z.string()).default([]),
  requires_totp: z.literal(false).optional(),
  token: z.string(),
})

export const loginResponseSchema = z.union([loginTotpRequiredSchema, loginSuccessSchema])

export type WireLoginResponse = z.infer<typeof loginResponseSchema>

// ── /auth/confirm ────────────────────────────────────────────────
// Backend: crates/webapi/src/session/confirm.rs — fn status, fn confirm

export const confirmStatusSchema = z.object({
  confirmed: z.boolean(),
  expires_in: z.number().nullable(),
})

export type WireConfirmStatus = z.infer<typeof confirmStatusSchema>

/** The body of a refused confirmation: 401 or 403 with a code, 429 with a sentence. */
export const confirmRefusalSchema = z.object({
  error: z.string(),
  remaining: z.number().optional(),
})

// ── /auth/totp/* ─────────────────────────────────────────────────

export const totpStatusSchema = z.object({
  enabled: z.boolean().default(false),
})

export type WireTotpStatus = z.infer<typeof totpStatusSchema>

/**
 * Backend `totp_setup` in `handlers::complete.rs` returns
 * `{secret, otpauth_url, recovery_codes}`. Frontend `_shared.tsx`
 * historically typed it as `{qr_url, recovery_codes, secret}` —
 * schema follows the backend truth; type in `_shared.tsx` will be
 * reconciled as follow-up.
 */
export const totpSetupSchema = z.object({
  otpauth_url: z.string(),
  recovery_codes: z.array(z.string()).default([]),
  secret: z.string(),
})

export type WireTotpSetup = z.infer<typeof totpSetupSchema>

// ── /auth/recovery-email ─────────────────────────────────────────
//
// Backend `get_recovery_email` (complete.rs) returns
// `{recovery_email: null | String}` — `null` when the user has no
// recovery email configured. Frontend schema previously required
// a string; Zod validation error would surface for every fresh
// account (2026-07-08 §10-audit).

export const recoveryEmailSchema = z
  .object({
    recovery_email: z.string().nullable().default(null),
  })
  .transform((v) => ({ recovery_email: v.recovery_email ?? '' }))

export type WireRecoveryEmail = z.infer<typeof recoveryEmailSchema>

// Re-export empty for auth mutations (logout / change-password /
// forgot-password / totp enable / totp disable / recovery-email set /
// recovery-email delete)
export { emptyResponseSchema }

/**
 * Backend: crates/webapi/src/handlers/external_login.rs.
 *
 * `provider` is derived from the issuer for display and may be null when a
 * link outlives the configuration that created it — the issuer is what is
 * stored, and an unconfigured one is still a real link the user should be
 * able to see and remove.
 */
export const externalProvidersSchema = z.object({
  providers: z.array(z.string()).default([]),
})

export const linkedIdentitySchema = z.object({
  issuer: z.string(),
  provider: z.string().nullish(),
  subject: z.string(),
})

export type WireLinkedIdentity = z.infer<typeof linkedIdentitySchema>

export const linkedIdentityListSchema = z.object({
  items: z.array(linkedIdentitySchema).default([]),
})
