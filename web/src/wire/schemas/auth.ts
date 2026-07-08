/**
 * Auth wire schemas — v2.1 §8 batch 2 (2026-07-08).
 *
 * OIDC config, login (with TOTP-required union branch), /auth/me
 * profile refresh, TOTP setup/status, recovery email get/set.
 */

import { z } from 'zod'

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
  permissions: z.array(z.string()).default([]),
})

export type WireAuthMe = z.infer<typeof authMeSchema>

// ── /auth/login — union: TOTP-required OR auth token ─────────────

const loginTotpRequiredSchema = z.object({
  requires_totp: z.literal(true),
})

const loginSuccessSchema = z.object({
  accessible_domains: z.array(z.string()).default([]),
  address: z.string(),
  display_name: z.string().default(''),
  permissions: z.array(z.string()).default([]),
  requires_totp: z.literal(false).optional(),
  token: z.string(),
})

export const loginResponseSchema = z.union([loginTotpRequiredSchema, loginSuccessSchema])

export type WireLoginResponse = z.infer<typeof loginResponseSchema>

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

export const recoveryEmailSchema = z.object({
  recovery_email: z.string(),
})

export type WireRecoveryEmail = z.infer<typeof recoveryEmailSchema>

// Re-export empty for auth mutations (logout / change-password /
// forgot-password / totp enable / totp disable / recovery-email set /
// recovery-email delete)
export { emptyResponseSchema }
