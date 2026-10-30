/**
 * Auth wire endpoints — v2.1 §8 batch 2 (2026-07-08).
 *
 * All auth flows funnel through here. `login` is the special case —
 * it returns a discriminated union of `{requires_totp: true}` OR
 * `{token, address, ...}`, so callers pattern-match on the response
 * shape rather than the HTTP status.
 */

import { http } from '@/lib/http'

import { wireFetch } from '../client'
import {
  authMeSchema,
  confirmRefusalSchema,
  confirmStatusSchema,
  emptyResponseSchema,
  externalProvidersSchema,
  linkedIdentityListSchema,
  loginResponseSchema,
  oidcConfigSchema,
  recoveryEmailSchema,
  resetCodeIssuedSchema,
  resetCodeStatusSchema,
  totpSetupSchema,
  totpStatusSchema,
  type WireAuthMe,
  type WireConfirmStatus,
  type WireLinkedIdentity,
  type WireLoginResponse,
  type WireOidcConfig,
  type WireRecoveryEmail,
  type WireResetCodeIssued,
  type WireResetCodeStatus,
  type WireTotpSetup,
  type WireTotpStatus,
} from '../schemas/auth'

// ── discovery + profile ─────────────────────────────────────────

export const wireGetOidcConfig = (): Promise<WireOidcConfig> =>
  wireFetch(oidcConfigSchema, { path: '/auth/oidc/config' })

/**
 * `GET /auth/me` through the session cookie. Plain `fetch` rather than
 * `wireFetch`: the login page calls it to ask whether a cookie session
 * exists, and a 401 there means "not yet", not "send to the login page".
 */
export async function wireGetMe(): Promise<WireAuthMe> {
  const res = await http('/api/auth/me')
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

export type WireConfirmOutcome =
  | { kind: 'code-needed' }
  | { kind: 'confirmed' }
  | { kind: 'refused'; message: string }
  | { kind: 'signed-out' }
  | { kind: 'wrong'; remaining: number }

// ── recent confirmation ────────────────────────────────────────

/**
 * Backend: crates/webapi/src/session/confirm.rs — fn confirm.
 *
 * 204 on success. A refusal is told apart by its code rather than thrown:
 * a wrong password says how many tries are left, a missing one-time code
 * asks for it, and the fifth wrong try ends the session.
 */
export async function wireConfirm(password: string, code?: string): Promise<WireConfirmOutcome> {
  const res = await http('/api/auth/confirm', {
    body: JSON.stringify(code === undefined ? { password } : { code, password }),
    headers: { 'Content-Type': 'application/json' },
    method: 'POST',
  })
  if (res.status === 204) return { kind: 'confirmed' }
  const parsed = confirmRefusalSchema.safeParse(await res.json().catch(() => null))
  const error = parsed.success ? parsed.data.error : `The server answered ${res.status}`
  switch (error) {
    case 'confirm_failed':
      return { kind: 'wrong', remaining: parsed.data?.remaining ?? 0 }
    case 'session_revoked':
      return { kind: 'signed-out' }
    case 'totp_required':
      return { kind: 'code-needed' }
    default:
      return { kind: 'refused', message: error }
  }
}

export async function wireSetRecoveryEmail(recoveryEmail: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { recovery_email: recoveryEmail },
    method: 'POST',
    path: '/auth/recovery-email',
  })
}

/** Backend: crates/webapi/src/session/confirm.rs — fn status. */
export const wireConfirmStatus = (): Promise<WireConfirmStatus> =>
  wireFetch(confirmStatusSchema, { path: '/auth/confirm' })

// ── TOTP ───────────────────────────────────────────────────────

export const wireGetTotpStatus = (): Promise<WireTotpStatus> =>
  wireFetch(totpStatusSchema, { path: '/auth/totp/status' })

export const wireTotpSetup = (): Promise<WireTotpSetup> =>
  wireFetch(totpSetupSchema, { body: {}, method: 'POST', path: '/auth/totp/setup' })

/**
 * Backend: crates/webapi/src/handlers/external_login.rs — `list_providers`.
 *
 * Which third-party sign-in methods this deployment has credentials for. A
 * provider with no client id configured is absent, so the login page offers
 * what actually works rather than buttons that lead to a 404.
 *
 * Unauthenticated: it is read before anybody has signed in and returns only
 * the keys, never a secret.
 */
export async function wireExternalProviders(): Promise<readonly string[]> {
  const raw = await wireFetch(externalProvidersSchema, { path: '/auth/external-providers' })
  return raw.providers
}

/**
 * Backend: `list_identities`. The sign-in methods linked to this account.
 */
export async function wireLinkedIdentities(): Promise<readonly WireLinkedIdentity[]> {
  const raw = await wireFetch(linkedIdentityListSchema, { path: '/auth/identities' })
  return raw.items
}

/** `POST /auth/password-reset` — a new password, with the code an issuer handed out. */
export async function wireResetPasswordWithCode(
  address: string,
  code: string,
  newPassword: string
): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { address, code, new_password: newPassword },
    method: 'POST',
    path: '/auth/password-reset',
  })
}

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

// ── reset codes ─────────────────────────────────────────────────

/**
 * Backend: `unlink_identity`. The account is the session's, never the body's.
 */
export async function wireUnlinkIdentity(issuer: string, subject: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { issuer, subject },
    method: 'POST',
    path: '/auth/identities:unlink',
  })
}

/** `POST /reset-codes/{address}` — issue a code; it is in this answer only. */
export const wireIssueResetCode = (address: string): Promise<WireResetCodeIssued> =>
  wireFetch(resetCodeIssuedSchema, {
    method: 'POST',
    path: `/reset-codes/${encodeURIComponent(address)}`,
  })

/** `GET /reset-codes/{address}` — whether a code is out, never the code. */
export const wireResetCodeStatus = (address: string): Promise<WireResetCodeStatus> =>
  wireFetch(resetCodeStatusSchema, { path: `/reset-codes/${encodeURIComponent(address)}` })
