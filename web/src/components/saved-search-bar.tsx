import { toast } from '@goliapkg/gds'
import { useAtom } from 'jotai'
import { Bookmark, ListFilter, Search, X } from 'lucide-react'
import { useState } from 'react'
import { useNavigate } from 'react-router'

import {
  useCreateSavedSearch,
  useDeleteSavedSearch,
  useSavedSearches,
} from '@/hooks/use-saved-searches'
import { searchQueryAtom } from '@/store/ui'

/** Shown while searching: makes a rule of what is in the box. */
export function CreateRuleButton() {
  const [query] = useAtom(searchQueryAtom)
  const navigate = useNavigate()
  return (
    <button
      aria-label="Create a rule from this search"
      className="text-fg-muted hover:bg-bg-secondary flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all duration-150"
      onClick={() => navigate(`/settings?tab=rules&rule_query=${encodeURIComponent(query.trim())}`)}
      title="Create a rule from this search"
      type="button"
    >
      <ListFilter aria-hidden="true" className="h-4 w-4" />
    </button>
  )
}

/** The user's saved searches under the labels; each runs its query. */
export function SavedSearchBar() {
  const { data } = useSavedSearches()
  const [query, setQuery] = useAtom(searchQueryAtom)
  const remove = useDeleteSavedSearch()
  const saved = data ?? []
  if (saved.length === 0) return null
  return (
    <div
      aria-label="Saved searches"
      className="border-border flex gap-1 overflow-x-auto border-b px-3 py-1.5 text-xs"
      role="toolbar"
    >
      {saved.map((s) => (
        <span className={chipClass(query.trim() === s.query)} key={s.id}>
          <button
            aria-pressed={query.trim() === s.query}
            className="flex items-center gap-1.5"
            onClick={() => setQuery(s.query)}
            title={s.query}
            type="button"
          >
            <Search aria-hidden="true" className="h-3 w-3" />
            <span className="max-w-32 truncate">{s.name}</span>
          </button>
          <button
            aria-label={`Delete saved search ${s.name}`}
            className="text-fg-muted hover:text-fg rounded"
            onClick={() => remove.mutate(s.id, { onError: (e) => toast.error(errorText(e)) })}
            type="button"
          >
            <X aria-hidden="true" className="h-3 w-3" />
          </button>
        </span>
      ))}
    </div>
  )
}

/** Shown while searching: saves what is in the box under a name. */
export function SaveSearchButton() {
  const [query] = useAtom(searchQueryAtom)
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const create = useCreateSavedSearch()
  const save = () => {
    create.mutate(
      { name, query },
      {
        onError: (e) => toast.error(errorText(e)),
        onSuccess: () => {
          toast.success('Search saved')
          setOpen(false)
        },
      }
    )
  }
  return (
    <div className="relative">
      <button
        aria-expanded={open}
        aria-label="Save this search"
        className="text-fg-muted hover:bg-bg-secondary flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-all duration-150"
        onClick={() => {
          setName(query.trim())
          setOpen(!open)
        }}
        title="Save this search"
        type="button"
      >
        <Bookmark aria-hidden="true" className="h-4 w-4" />
      </button>
      {open && (
        <form
          className="bg-surface border-border absolute top-9 right-0 z-20 flex w-64 flex-col gap-2 rounded-xl border p-3 shadow-lg"
          onSubmit={(e) => {
            e.preventDefault()
            save()
          }}
        >
          <label className="text-fg-secondary text-xs" htmlFor="saved-search-name">
            Name
          </label>
          <input
            autoFocus
            className="border-border bg-bg text-fg focus:border-accent rounded-md border px-2 py-1 text-sm outline-none"
            id="saved-search-name"
            maxLength={64}
            onChange={(e) => setName(e.target.value)}
            value={name}
          />
          <div className="flex justify-end gap-2">
            <button
              className="text-fg-secondary hover:bg-bg-secondary rounded-md px-2 py-1 text-xs"
              onClick={() => setOpen(false)}
              type="button"
            >
              Cancel
            </button>
            <button
              className="bg-accent text-accent-fg rounded-md px-2 py-1 text-xs disabled:opacity-50"
              disabled={name.trim().length === 0 || create.isPending}
              type="submit"
            >
              Save
            </button>
          </div>
        </form>
      )}
    </div>
  )
}

function chipClass(active: boolean): string {
  const base = 'flex shrink-0 items-center gap-1.5 rounded-md px-2 py-0.5 transition-colors'
  if (active) return `${base} bg-bg-tertiary text-fg font-medium`
  return `${base} text-fg-secondary hover:bg-bg-secondary`
}

function errorText(err: unknown): string {
  if (err instanceof Error) return err.message
  return 'Something went wrong'
}
