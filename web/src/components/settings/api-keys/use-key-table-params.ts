import type { SortDir, SortField } from './table-model'

import { useSearchParams } from 'react-router'

import { parsePageSize, parseSortDir, parseSortField } from './table-model'

export type KeyTableParams = {
  dir: SortDir
  page: number
  query: string
  size: number
  sort: SortField
}

export const TABLE_PARAM_NAMES = ['dir', 'page', 'q', 'size', 'sort'] as const

/**
 * Search / sort / page live in the URL, per `rules/typescript/patterns.md`
 * ("URL as State") — a link to a filtered, sorted page reopens on the same
 * view. `replace: true` keeps table fiddling out of the back-button history.
 */
export function useKeyTableParams(): {
  params: KeyTableParams
  setPage: (page: number) => void
  setQuery: (query: string) => void
  setSize: (size: number) => void
  toggleSort: (field: SortField) => void
} {
  const [searchParams, setSearchParams] = useSearchParams()

  const params: KeyTableParams = {
    dir: parseSortDir(searchParams.get('dir')),
    page: parsePage(searchParams.get('page')),
    query: searchParams.get('q') ?? '',
    size: parsePageSize(searchParams.get('size')),
    sort: parseSortField(searchParams.get('sort')),
  }

  const patch = (values: Record<string, null | string>) => {
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev)
        for (const [name, value] of Object.entries(values)) {
          if (value === null) next.delete(name)
          else next.set(name, value)
        }
        return next
      },
      { replace: true }
    )
  }

  return {
    params,
    setPage: (page: number) => patch({ page: String(page) }),
    // any change to the result set invalidates the current page number
    setQuery: (query: string) => patch({ page: null, q: emptyToNull(query) }),
    setSize: (size: number) => patch({ page: null, size: String(size) }),
    toggleSort: (field: SortField) => {
      if (field === params.sort) {
        patch({ dir: flip(params.dir) })
        return
      }
      patch({ dir: defaultDir(field), page: null, sort: field })
    },
  }
}

/** Newest-first and highest-id-first read naturally; text reads A→Z. */
function defaultDir(field: SortField): SortDir {
  if (field === 'created' || field === 'id') return 'desc'
  return 'asc'
}

function emptyToNull(value: string): null | string {
  if (value.trim().length === 0) return null
  return value
}

function flip(dir: SortDir): SortDir {
  if (dir === 'asc') return 'desc'
  return 'asc'
}

function parsePage(raw: null | string): number {
  const n = Number(raw)
  if (!Number.isFinite(n) || n < 1) return 1
  return Math.floor(n)
}
