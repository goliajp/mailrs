/**
 * Admin resource wire endpoints — v2.1 §10.3 (2026-07-08).
 *
 * Uniform CRUD helpers over permissive schemas. Every admin resource
 * (accounts / aliases / apps / domains / email-groups / groups /
 * greylist / permissions / config / audit-log / mail-audit / overview
 * / queue) uses `adminListGet` / `adminPost` / `adminPatch` /
 * `adminPut` / `adminDelete` — no per-resource typing at the wire
 * boundary. Downstream keeps the `Record<string, unknown>` shape and
 * casts to its local resource type on read; when a shape drifts, the
 * downstream cast fails visibly rather than silently corrupting data.
 */

import { wireFetch } from '../client'
import {
  adminEmptyResultSchema,
  adminListSchema,
  adminObjectSchema,
  adminResultSchema,
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

// ── mutations ──────────────────────────────────────────────────

export async function adminObjectGet<T = AnyObj>(path: string, signal?: AbortSignal): Promise<T> {
  return wireFetch(adminObjectSchema, { path, signal }) as unknown as Promise<T>
}

export async function adminPatch<T = AnyObj>(path: string, body?: unknown): Promise<T> {
  return wireFetch(adminResultSchema, { body, method: 'PATCH', path }) as unknown as Promise<T>
}

export async function adminPost<T = AnyObj>(path: string, body?: unknown): Promise<T> {
  return wireFetch(adminResultSchema, { body, method: 'POST', path }) as unknown as Promise<T>
}

export async function adminPut<T = AnyObj>(path: string, body?: unknown): Promise<T> {
  return wireFetch(adminResultSchema, { body, method: 'PUT', path }) as unknown as Promise<T>
}
