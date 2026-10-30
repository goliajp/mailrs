/**
 * Admin resource wire endpoints — v2.1 §10.3 (2026-07-08).
 *
 * Uniform CRUD helpers over permissive schemas. Every admin resource
 * (accounts / aliases / domains / email-groups / groups /
 * greylist / permissions / config / audit-log / mail-audit / overview
 * / queue) uses `adminListGet` / `adminPost` / `adminPatch` /
 * `adminPut` / `adminDelete` — no per-resource typing at the wire
 * boundary. Downstream keeps the `Record<string, unknown>` shape and
 * casts to its local resource type on read; when a shape drifts, the
 * downstream cast fails visibly rather than silently corrupting data.
 *
 * That last sentence holds for the **read** direction only, and was taken
 * for the whole boundary until 2026-07-31. A cast on read says nothing
 * about a request body: `adminPost(path, {...})` takes an untyped object,
 * so a misspelled field compiles, serde drops it, and the write succeeds
 * having ignored it. Thirteen admin writes were unchecked on that basis.
 * `request-coverage.test.ts` now enumerates them and each is either pinned
 * by a fixture or listed as debt with a reason.
 */

import { wireFetch } from '../client'
import {
  adminEmptyResultSchema,
  adminListSchema,
  adminObjectSchema,
  adminStringListSchema,
  adminWriteResultSchema,
  passwordClearPreviewSchema,
  passwordClearResultSchema,
  type WirePasswordClearPreview,
  type WirePasswordClearResult,
} from '../schemas/admin'

type AnyObj = Record<string, unknown>

// ── list read ──────────────────────────────────────────────────

export async function adminDelete(path: string): Promise<void> {
  await wireFetch(adminEmptyResultSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path,
  })
}

// ── object read ────────────────────────────────────────────────

export async function adminListGet<T = AnyObj>(path: string, signal?: AbortSignal): Promise<T[]> {
  const raw = await wireFetch(adminListSchema, { path, signal })
  return raw.items as unknown as T[]
}

export async function adminObjectGet<T = AnyObj>(path: string, signal?: AbortSignal): Promise<T> {
  return wireFetch(adminObjectSchema, { path, signal }) as unknown as Promise<T>
}

// ── mutations ──────────────────────────────────────────────────

export async function adminPatch<T = AnyObj>(path: string, body?: unknown): Promise<T> {
  return wireFetch(adminWriteResultSchema, {
    allowEmpty: true,
    body,
    method: 'PATCH',
    path,
  }) as unknown as Promise<T>
}

export async function adminPost<T = AnyObj>(path: string, body?: unknown): Promise<T> {
  return wireFetch(adminWriteResultSchema, {
    allowEmpty: true,
    body,
    method: 'POST',
    path,
  }) as unknown as Promise<T>
}

export async function adminPut<T = AnyObj>(path: string, body?: unknown): Promise<T> {
  return wireFetch(adminWriteResultSchema, {
    allowEmpty: true,
    body,
    method: 'PUT',
    path,
  }) as unknown as Promise<T>
}

/** A list of strings (permissions, members, group names), however the endpoint wraps it. */
export async function adminStringListGet(path: string, signal?: AbortSignal): Promise<string[]> {
  return wireFetch(adminStringListSchema, { path, signal })
}

/** `POST /admin/password-clear/preview` — what clearing these accounts would cut off. */
export const wirePasswordClearPreview = (addresses: string[]): Promise<WirePasswordClearPreview> =>
  wireFetch(passwordClearPreviewSchema, {
    body: { addresses },
    method: 'POST',
    path: '/admin/password-clear/preview',
  })

/** `POST /admin/password-clear` — clear these accounts' passwords and end their sessions. */
export const wirePasswordClear = (addresses: string[]): Promise<WirePasswordClearResult> =>
  wireFetch(passwordClearResultSchema, {
    body: { addresses },
    method: 'POST',
    path: '/admin/password-clear',
  })
