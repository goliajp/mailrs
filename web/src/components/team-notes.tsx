import type { TeamConv } from '@/wire/endpoints/shared'
import type { WireNote } from '@/wire/schemas/shared'

import { toast } from '@goliapkg/gds'
import { Lock } from 'lucide-react'
import { useState } from 'react'

import { teamKeys, useTeamNotes } from '@/hooks/use-team'
import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { formatDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { wireAddNote, wireDeleteNote, wireEditNote } from '@/wire/endpoints/shared'

/**
 * The team's internal notes on a conversation. Shown apart from the mail
 * and marked as the team's only: a note is never part of a reply, a
 * forward or anything else that is sent.
 */
export function TeamNotes({ c, me }: { c: TeamConv; me: string }) {
  const t = useT()
  const { data: notes = [] } = useTeamNotes(c)
  const [draft, setDraft] = useState('')
  const refresh = () => queryClient.invalidateQueries({ queryKey: teamKeys.notes(c) })
  const add = async () => {
    const text = draft.trim()
    if (!text) return
    try {
      await wireAddNote(c, text)
      setDraft('')
      await refresh()
    } catch (e) {
      toast.error(errorText(e, t('team.notes.failed')))
    }
  }
  return (
    <div className="border-warning/40 bg-warning/5 space-y-2 rounded-md border border-dashed p-2">
      <p className="text-warning flex items-center gap-1.5 text-xs font-medium">
        <Lock aria-hidden className="h-3 w-3" />
        {t('team.notes')} · {t('team.notes.only')}
      </p>
      {notes.length === 0 && <p className="text-fg-muted text-xs">{t('team.notes.empty')}</p>}
      <ul className="space-y-1.5">
        {notes.map((n) => (
          <NoteItem c={c} key={n.id} mine={n.by === me} note={n} onChanged={refresh} />
        ))}
      </ul>
      <div className="flex gap-2">
        <textarea
          aria-label={t('team.notes.placeholder')}
          className="border-border bg-bg min-h-[2.5rem] flex-1 rounded-md border px-2 py-1 text-xs"
          onChange={(e) => setDraft(e.target.value)}
          placeholder={t('team.notes.placeholder')}
          value={draft}
        />
        <button
          className="bg-fg text-bg self-end rounded-md px-2.5 py-1 text-xs disabled:opacity-50"
          disabled={!draft.trim()}
          onClick={() => void add()}
          type="button"
        >
          {t('team.notes.add')}
        </button>
      </div>
    </div>
  )
}

function NoteItem({
  c,
  mine,
  note,
  onChanged,
}: {
  c: TeamConv
  mine: boolean
  note: WireNote
  onChanged: () => Promise<void>
}) {
  const t = useT()
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState(note.text)
  const run = async (f: () => Promise<void>) => {
    try {
      await f()
      setEditing(false)
      await onChanged()
    } catch (e) {
      toast.error(errorText(e, t('team.notes.failed')))
    }
  }
  const remove = () => {
    if (!window.confirm(t('team.notes.deleteConfirm'))) return
    void run(() => wireDeleteNote(c, note.id))
  }
  return (
    <li className="text-xs">
      <div className="text-fg-muted flex items-center gap-2">
        <span className="text-fg-secondary font-medium">{note.by}</span>
        <span>{formatDate(note.at)}</span>
        {note.edited_at !== null && <span>· {t('team.notes.edited')}</span>}
        {mine && !editing && (
          <span className="ml-auto flex gap-2">
            <button className="text-accent" onClick={() => setEditing(true)} type="button">
              {t('team.notes.edit')}
            </button>
            <button className="text-danger" onClick={remove} type="button">
              {t('team.notes.delete')}
            </button>
          </span>
        )}
      </div>
      {!editing && <p className="text-fg whitespace-pre-wrap">{note.text}</p>}
      {editing && (
        <div className="mt-1 flex gap-2">
          <textarea
            aria-label={t('team.notes.edit')}
            className="border-border bg-bg flex-1 rounded-md border px-2 py-1"
            onChange={(e) => setText(e.target.value)}
            value={text}
          />
          <button
            className="text-accent self-end"
            onClick={() => void run(() => wireEditNote(c, note.id, text.trim()))}
            type="button"
          >
            {t('team.notes.save')}
          </button>
          <button
            className="text-fg-muted self-end"
            onClick={() => setEditing(false)}
            type="button"
          >
            {t('team.notes.cancel')}
          </button>
        </div>
      )}
    </li>
  )
}
