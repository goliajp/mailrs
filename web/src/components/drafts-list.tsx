import type { ContextMenuItem } from '@/components/context-menu'
import type { Draft } from '@/lib/api'

import { toast } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { Trash2 } from 'lucide-react'
import { memo, useMemo, useState } from 'react'

import { ActionSheet, ContextMenu, useContextMenu } from '@/components/context-menu'
import { DateDivider } from '@/components/conversation-list'
import { FilterBar } from '@/components/conversation-list-filter-bar'
import { ListSearchInput } from '@/components/list-search-input'
import { useDeleteDraftMutation, useDraftsQuery } from '@/hooks/use-drafts'
import { dateGroupLabel, formatFullDate } from '@/lib/format'
import { composeDraftSourceAtom, composeReplySourceAtom, composingNewAtom } from '@/store/ui'

// rows interleaved with Today / Yesterday / weekday group pills, same
// grouping the inbox list uses (drafts group on updated_at).
type DraftListItem = { draft: Draft; type: 'row' } | { label: string; type: 'divider' }

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
        {/* h-16 two-line rows + 3px left border, matching ROW_BASE in
            conversation-list so every list shares one row height. */}
        {groupByDate(filtered).map((item) => {
          if (item.type === 'divider') {
            return <DateDivider key={`d:${item.label}`} label={item.label} />
          }
          return (
            <DraftRow
              draft={item.draft}
              key={item.draft.id}
              onDelete={removeDraft}
              onOpen={openDraft}
            />
          )
        })}
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

// One draft row + its context menu. Same pattern as SentRow /
// ConversationItem: right-click / long-press → Open + Delete.
// Hover-to-reveal trash button kept for pointer-only flows; the
// context menu is the discoverable, mobile-safe path.
const DraftRow = memo(function DraftRow({
  draft,
  onDelete,
  onOpen,
}: {
  draft: Draft
  onDelete: (id: number) => void
  onOpen: (d: Draft) => void
}) {
  const ctx = useContextMenu()
  const items = useMemo<ContextMenuItem[]>(
    () => [
      {
        label: 'Open',
        onClick: () => onOpen(draft),
      },
      {
        danger: true,
        label: 'Delete',
        onClick: () => onDelete(Number(draft.id)),
      },
    ],
    [draft, onOpen, onDelete]
  )

  return (
    <div
      className="hover:bg-bg-secondary group relative h-16 border-l-[3px] border-l-transparent"
      onTouchEnd={ctx.onTouchEnd}
      onTouchMove={ctx.onTouchMove}
      onTouchStart={ctx.onTouchStart}
      role="listitem"
    >
      <button
        className="flex h-full w-full flex-col justify-center gap-1 px-4 py-2 pr-10 text-left"
        onClick={() => onOpen(draft)}
        onContextMenu={ctx.open}
        type="button"
      >
        <div className="flex items-center justify-between gap-2">
          <span className="text-fg-secondary truncate text-sm font-medium">
            {draftTitle(draft.subject)}
          </span>
          <span className="text-fg-muted text-tiny shrink-0">
            {formatFullDate(Number(draft.updated_at))}
          </span>
        </div>
        <span className="text-fg-muted truncate text-sm">
          To: {draft.to || '—'} · {draftPreview(draft.body)}
        </span>
      </button>
      <button
        aria-label="Delete draft"
        className="text-fg-muted hover:text-danger absolute top-1/2 right-3 -translate-y-1/2 opacity-0 transition-opacity group-hover:opacity-100"
        onClick={() => onDelete(Number(draft.id))}
        type="button"
      >
        <Trash2 className="h-4 w-4" />
      </button>
      <ContextMenu items={items} onClose={ctx.close} position={ctx.position} />
      <ActionSheet items={items} onClose={ctx.close} open={ctx.actionSheetOpen} />
    </div>
  )
})

function draftPreview(body: string): string {
  const flat = body.replace(/\s+/g, ' ').trim()
  if (flat.length <= 120) return flat
  return `${flat.slice(0, 120)}…`
}

function draftTitle(subject: string): string {
  if (subject.trim()) return subject
  return '(no subject)'
}

function groupByDate(drafts: readonly Draft[]): DraftListItem[] {
  const out: DraftListItem[] = []
  let prev = ''
  for (const d of drafts) {
    const label = dateGroupLabel(Number(d.updated_at))
    if (label !== prev) {
      out.push({ label, type: 'divider' })
      prev = label
    }
    out.push({ draft: d, type: 'row' })
  }
  return out
}

function matchesDraft(d: Draft, q: string): boolean {
  return (
    d.subject.toLowerCase().includes(q) ||
    d.to.toLowerCase().includes(q) ||
    d.body.toLowerCase().includes(q)
  )
}
