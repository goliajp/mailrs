import type { Draft } from '@/lib/api'

import { toast } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { Trash2 } from 'lucide-react'
import { useMemo, useState } from 'react'

import { FilterBar } from '@/components/conversation-list-filter-bar'
import { ListSearchInput } from '@/components/list-search-input'
import { useDeleteDraftMutation, useDraftsQuery } from '@/hooks/use-drafts'
import { formatFullDate } from '@/lib/format'
import { composeDraftSourceAtom, composeReplySourceAtom, composingNewAtom } from '@/store/ui'

// server-backed drafts list, shown when the Draft tab is active. clicking
// a row reopens it in the composer (which upserts the same id on autosave
// and deletes it on send).
export function DraftsList() {
  const { data: drafts = [], isLoading } = useDraftsQuery()
  const deleteDraftMut = useDeleteDraftMutation()
  const setComposingNew = useSetAtom(composingNewAtom)
  const setDraftSource = useSetAtom(composeDraftSourceAtom)
  const setReplySource = useSetAtom(composeReplySourceAtom)
  const [query, setQuery] = useState('')

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return drafts
    return drafts.filter((d) => matchesDraft(d, q))
  }, [drafts, query])

  const openDraft = (d: Draft) => {
    setReplySource(null)
    setDraftSource({
      bcc: d.bcc,
      body: d.body,
      cc: d.cc,
      id: Number(d.id),
      subject: d.subject,
      to: d.to,
    })
    setComposingNew(true)
  }

  const removeDraft = (id: number) => {
    deleteDraftMut.mutate(id, {
      onError: () => toast.error('Could not delete draft'),
    })
  }

  // the list body varies by load/empty/populated; the FilterBar (tab
  // navigation) must stay mounted above it in every state so switching
  // away from the Draft tab is always possible.
  const renderBody = () => {
    if (isLoading) {
      return <div className="text-fg-muted p-4 text-xs">Loading drafts…</div>
    }
    if (drafts.length === 0) {
      return <div className="text-fg-muted p-8 text-center text-sm">No drafts</div>
    }
    if (filtered.length === 0) {
      return <div className="text-fg-muted p-8 text-center text-sm">No matching drafts</div>
    }
    return (
      <div className="flex flex-col">
        {filtered.map((d) => (
          <div
            className="border-border hover:bg-bg-secondary group relative border-b border-l-[3px] border-l-transparent"
            key={d.id}
          >
            <button
              className="flex w-full flex-col gap-1 px-4 py-3 pr-10 text-left"
              onClick={() => openDraft(d)}
              type="button"
            >
              <span className="text-fg truncate text-sm font-medium">{draftTitle(d.subject)}</span>
              <span className="text-fg-secondary truncate text-xs">To: {d.to || '—'}</span>
              <span className="text-fg-muted truncate text-xs">{draftPreview(d.body)}</span>
              <span className="text-fg-muted text-tiny">
                {formatFullDate(Number(d.updated_at))}
              </span>
            </button>
            <button
              aria-label="Delete draft"
              className="text-fg-muted hover:text-danger absolute top-3 right-3 opacity-0 transition-opacity group-hover:opacity-100"
              onClick={() => removeDraft(Number(d.id))}
              type="button"
            >
              <Trash2 className="h-4 w-4" />
            </button>
          </div>
        ))}
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      <ListSearchInput
        label="Search drafts"
        onChange={setQuery}
        placeholder="Search drafts…"
        value={query}
      />
      <FilterBar />
      <div className="min-h-0 flex-1 overflow-y-auto">{renderBody()}</div>
    </div>
  )
}

function draftPreview(body: string): string {
  const flat = body.replace(/\s+/g, ' ').trim()
  if (flat.length <= 120) return flat
  return `${flat.slice(0, 120)}…`
}

function draftTitle(subject: string): string {
  if (subject.trim()) return subject
  return '(no subject)'
}

function matchesDraft(d: Draft, q: string): boolean {
  return (
    d.subject.toLowerCase().includes(q) ||
    d.to.toLowerCase().includes(q) ||
    d.body.toLowerCase().includes(q)
  )
}
