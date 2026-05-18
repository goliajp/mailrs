import type { ConversationSummary } from '@/lib/types'

// Stale-while-revalidate cache for the conversation list.
//
// Why: on a refresh, the network round-trip + JSON parse + render leaves the
// UI showing a skeleton for ~300-600ms even on fast connections — long
// enough to feel "webapp-y". Stashing the last response per filter-path in
// localStorage lets us paint the previous content immediately on mount,
// then silently replace it when the fresh fetch lands.
//
// Storage shape: single localStorage key per user holding an LRU map of
// path → { data, savedAt }. We cap to MAX_PATHS entries (last-touched wins).
// TTL is just a guard against ancient entries; we always re-fetch in the
// background anyway, so even slightly-stale data is fine.

const STORAGE_KEY = 'mailrs:list-cache:v1'
const MAX_PATHS = 8
const TTL_MS = 24 * 60 * 60 * 1000

type CacheShape = {
  paths: Record<string, Entry>
  user: string
}

type Entry = {
  data: ConversationSummary[]
  savedAt: number
}

export function clearListCache(): void {
  try {
    localStorage.removeItem(STORAGE_KEY)
  } catch {
    // ignore
  }
}

export function readListCache(user: string, path: string): ConversationSummary[] | null {
  const cache = readRaw()
  if (!cache || cache.user !== user) return null
  const entry = cache.paths[path]
  if (!entry) return null
  if (Date.now() - entry.savedAt > TTL_MS) return null
  return entry.data
}

export function writeListCache(user: string, path: string, data: ConversationSummary[]): void {
  const existing = readRaw()
  const shape: CacheShape = existing && existing.user === user ? existing : { paths: {}, user }
  shape.paths[path] = { data, savedAt: Date.now() }
  // LRU trim: drop oldest entries until under MAX_PATHS
  const keys = Object.keys(shape.paths)
  if (keys.length > MAX_PATHS) {
    keys
      .map((k) => ({ k, t: shape.paths[k].savedAt }))
      .sort((a, b) => a.t - b.t)
      .slice(0, keys.length - MAX_PATHS)
      .forEach(({ k }) => {
        delete shape.paths[k]
      })
  }
  writeRaw(shape)
}

function readRaw(): CacheShape | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as CacheShape
    if (!parsed || typeof parsed !== 'object' || !parsed.paths) return null
    return parsed
  } catch {
    return null
  }
}

function writeRaw(value: CacheShape) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(value))
  } catch {
    // quota or privacy mode — silently degrade
  }
}
