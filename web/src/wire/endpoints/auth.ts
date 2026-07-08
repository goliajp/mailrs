/**
 * Auth wire endpoints — v2.1 §8 batch 2 (2026-07-08).
 *
 * All auth flows funnel through here. `login` is the special case —
 * it returns a discriminated union of `{requires_totp: true}` OR
 * `{token, address, ...}`, so callers pattern-match on the response
 * shape rather than the HTTP status.
 */

import { wireFetch } from '../client'
import {
  authMeSchema,
  emptyResponseSchema,
  loginResponseSchema,
  oidcConfigSchema,
  recoveryEmailSchema,
  totpSetupSchema,
  totpStatusSchema,
  type WireAuthMe,
  type WireLoginResponse,
  type WireOidcConfig,
  type WireRecoveryEmail,
  type WireTotpSetup,
  type WireTotpStatus,
} from '../schemas/auth'

// ── discovery + profile ─────────────────────────────────────────

export const wireGetOidcConfig = (): Promise<WireOidcConfig> =>
  wireFetch(oidcConfigSchema, { path: '/auth/oidc/config' })

/**
 * `GET /auth/me` uses an explicit bearer token (not the ambient
 * `getToken()` from auth store) because it's called on the OIDC
 * callback right after the auth atom is set — the store might not
 * have propagated to `getToken()` yet.
 */
export async function wireGetMe(token: string): Promise<WireAuthMe> {
  const res = await fetch('/api/auth/me', {
    headers: { Authorization: `Bearer ${token}` },
  })
  if (!res.ok) throw new Error(`auth/me ${res.status}`)
  const raw = await res.json()
  return authMeSchema.parse(raw)
}

// ── login / logout ─────────────────────────────────────────────

/**
 * `POST /auth/login` — returns TOTP-required union or auth token.
 * Caller pattern-matches on `response.requires_totp`.
 */
export const wireLogin = (
  address: string,
  password: string,
  totpCode?: string
): Promise<WireLoginResponse> =>
  wireFetch(loginResponseSchema, {
    body: {
      address,
      password,
      ...(totpCode ? { totp_code: totpCode } : {}),
    },
    method: 'POST',
    path: '/auth/login',
  })

export async function wireChangePassword(
  currentPassword: string,
  newPassword: string
): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { current_password: currentPassword, new_password: newPassword },
    method: 'POST',
    path: '/auth/change-password',
  })
}

// ── password / recovery ────────────────────────────────────────

/**
 * `POST /auth/forgot-password` — backend only requires `address`,
 * `recovery_email` is accepted but ignored by the current backend
 * (`ForgotPasswordRequest` only has `address`). Kept in the wire
 * layer for future compat.
 */
export async function wireForgotPassword(address: string, recoveryEmail?: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: recoveryEmail ? { address, recovery_email: recoveryEmail } : { address },
    method: 'POST',
    path: '/auth/forgot-password',
  })
}

export async function wireLogout(): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: {},
    method: 'POST',
    path: '/auth/logout',
  })
}

export async function wireResetPassword(token: string, newPassword: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { new_password: newPassword, token },
    method: 'POST',
    path: '/auth/reset-password',
  })
}

export const wireGetRecoveryEmail = (): Promise<WireRecoveryEmail> =>
  wireFetch(recoveryEmailSchema, { path: '/auth/recovery-email' })

export async function wireSetRecoveryEmail(recoveryEmail: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { recovery_email: recoveryEmail },
    method: 'POST',
    path: '/auth/recovery-email',
  })
}

// ── TOTP ───────────────────────────────────────────────────────

export const wireGetTotpStatus = (): Promise<WireTotpStatus> =>
  wireFetch(totpStatusSchema, { path: '/auth/totp/status' })

export const wireTotpSetup = (): Promise<WireTotpSetup> =>
  wireFetch(totpSetupSchema, { body: {}, method: 'POST', path: '/auth/totp/setup' })

export async function wireTotpDisable(code: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { code },
    method: 'POST',
    path: '/auth/totp/disable',
  })
}

export async function wireTotpEnable(code: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { code },
    method: 'POST',
    path: '/auth/totp/enable',
  })
}
