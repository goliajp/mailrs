import type { WireStorage } from '@/wire/schemas/storage'

export const storageKey = ['storage'] as const

/** From this share of the quota on, the user is told it is filling up. */
const NEAR = 0.9

export type StorageLevel = 'near' | 'ok' | 'over'

/**
 * Where the usage stands against the quota. No quota means nothing to
 * warn about. Only imports stop at the quota: sending always goes out,
 * so the count can pass it.
 */
export function storageLevel(storage: null | undefined | WireStorage): StorageLevel {
  const quota = storage?.quota_bytes
  if (!storage || quota === null || quota === undefined || quota <= 0) return 'ok'
  if (storage.used_bytes >= quota) return 'over'
  if (storage.used_bytes >= quota * NEAR) return 'near'
  return 'ok'
}
