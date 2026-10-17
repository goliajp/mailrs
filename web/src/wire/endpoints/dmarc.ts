/**
 * DMARC aggregate-report wire endpoints.
 *
 * Unlike the other admin resources (which share the permissive
 * `adminListGet` helper), these are typed at the boundary: the
 * endpoints are new, the Rust structs are fixed, and the UI reads
 * specific numeric fields where a silent shape drift would show up as
 * a wrong number rather than a missing row.
 *
 * Backend: crates/webapi/src/handlers/dmarc.rs
 */

import type { DmarcSourceList } from '../schemas/dmarc'

import { wireFetch } from '../client'
import { wireDmarcReportListSchema, wireDmarcSourceListSchema } from '../schemas/dmarc'

/** `GET /api/admin/dmarc/reports` — newest reporting window first. */
export async function fetchDmarcReports(limit?: number, signal?: AbortSignal) {
  const raw = await wireFetch(wireDmarcReportListSchema, {
    path: `/admin/dmarc/reports${limitQuery(limit)}`,
    signal,
  })
  return raw.items
}

/** `GET /api/admin/dmarc/sources` — per-source-IP rollup. */
export async function fetchDmarcSources(
  domain?: string,
  days?: number,
  signal?: AbortSignal
): Promise<DmarcSourceList> {
  return wireFetch(wireDmarcSourceListSchema, {
    path: `/admin/dmarc/sources${sourcesQuery(domain, days)}`,
    signal,
  })
}

/** Build the `?limit=` query, or an empty string when unset. */
function limitQuery(limit?: number): string {
  if (limit === undefined) return ''
  return `?limit=${String(limit)}`
}

/** Build the sources query from an optional domain filter and window. */
function sourcesQuery(domain?: string, days?: number): string {
  const parts: string[] = []
  if (domain !== undefined && domain !== '') parts.push(`domain=${encodeURIComponent(domain)}`)
  if (days !== undefined) parts.push(`days=${String(days)}`)
  if (parts.length === 0) return ''
  return `?${parts.join('&')}`
}
