/**
 * Pure table model for the API Keys table — filter / sort / page.
 *
 * Kept free of React so the ordering rules (in particular the total-order
 * tie-break, which is what makes paging stable) are unit-testable without
 * mounting anything.
 */

import type { AgentKey } from '../_shared'

export type SortDir = 'asc' | 'desc'

export type SortField = 'created' | 'id' | 'name' | 'prefix' | 'scopes'

export const PAGE_SIZES = [10, 25, 50, 100] as const

export const SORT_FIELDS: readonly SortField[] = ['created', 'id', 'name', 'prefix', 'scopes']

/** Page numbers are 1-based everywhere, including in the URL. */
export function clampPage(page: number, total: number, size: number): number {
  const last = pageCount(total, size)
  if (!Number.isFinite(page) || page < 1) return 1
  if (page > last) return last
  return Math.floor(page)
}

/** Case-insensitive substring match over every field a human would search. */
export function filterKeys(keys: readonly AgentKey[], query: string): AgentKey[] {
  const needle = query.trim().toLowerCase()
  if (needle.length === 0) return [...keys]
  return keys.filter((key) => haystack(key).includes(needle))
}

export function pageCount(total: number, size: number): number {
  if (size <= 0) return 1
  return Math.max(1, Math.ceil(total / size))
}

export function pageSlice<T>(rows: readonly T[], page: number, size: number): T[] {
  const start = (clampPage(page, rows.length, size) - 1) * size
  return rows.slice(start, start + size)
}

export function parsePageSize(raw: null | string): number {
  const n = Number(raw)
  const found = PAGE_SIZES.find((size) => size === n)
  if (found) return found
  return 25
}

/** Parses a value that may be absent or malformed into a sortable number. */
export function parseSortDir(raw: null | string): SortDir {
  if (raw === 'asc') return 'asc'
  return 'desc'
}

export function parseSortField(raw: null | string): SortField {
  const found = SORT_FIELDS.find((field) => field === raw)
  if (found) return found
  return 'created'
}

export function sortKeys(keys: readonly AgentKey[], field: SortField, dir: SortDir): AgentKey[] {
  const factor = directionFactor(dir)
  return [...keys].sort((a, b) => {
    const primary = compareBy(field, a, b)
    if (primary !== 0) return primary * factor
    // total order or paging breaks: `created_at` is whole seconds, so keys
    // provisioned by a script collide. Ties resolve on the id, which is a
    // per-account monotonic counter — never equal for two distinct rows.
    return idValue(a) - idValue(b)
  })
}

function compareBy(field: SortField, a: AgentKey, b: AgentKey): number {
  switch (field) {
    case 'created':
      return createdValue(a) - createdValue(b)
    case 'id':
      return idValue(a) - idValue(b)
    case 'name':
      return a.name.localeCompare(b.name)
    case 'prefix':
      return a.prefix.localeCompare(b.prefix)
    case 'scopes':
      return a.scopes.join(',').localeCompare(b.scopes.join(','))
  }
}

function createdValue(key: AgentKey): number {
  const n = Number(key.created_at)
  if (Number.isFinite(n)) return n
  return 0
}

function directionFactor(dir: SortDir): number {
  if (dir === 'asc') return 1
  return -1
}

function haystack(key: AgentKey): string {
  return [key.name, key.prefix, key.id, ...key.scopes].join(' ').toLowerCase()
}

function idValue(key: AgentKey): number {
  const n = Number(key.id)
  if (Number.isFinite(n)) return n
  return 0
}
