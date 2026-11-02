import type { TeamConv } from '@/wire/endpoints/shared'
import type { WireSharedOverview } from '@/wire/schemas/shared'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
import { Users } from 'lucide-react'
import { useMemo, useState } from 'react'

import { TeamNotes } from '@/components/team-notes'
import {
  teamComposingAtom,
  teamConvOf,
  teamKeys,
  usePresence,
  useTeamOverview,
} from '@/hooks/use-team'
import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { formatDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { mailKeys } from '@/lib/query-keys'
import { authAtom } from '@/store/auth'
import { conversationKeys } from '@/store/query-keys-v21'
import { wireAssignShared, wireSharedDraft, wireTakeOverDraft } from '@/wire/endpoints/shared'

/**
 * A team inbox conversation's shared side, above the reply: who answers
 * it, who else has it open or is writing, the reply being written, and the
 * team's internal notes. Nothing here is shown for an ordinary
 * conversation.
 */
export function TeamPanel({ threadId }: { threadId: string }) {
  const c = useMemo(() => teamConvOf(threadId), [threadId])
  if (!c) return null
  return <TeamPanelFor c={c} />
}

function AssigneePicker({
  c,
  me,
  overview,
}: {
  c: TeamConv
  me: string
  overview: WireSharedOverview
}) {
  const t = useT()
  const [busy, setBusy] = useState(false)
  const assign = async (to: null | string) => {
    setBusy(true)
    try {
      await wireAssignShared(c, to)
      await queryClient.invalidateQueries({ queryKey: teamKeys.overview(c) })
      void queryClient.invalidateQueries({ queryKey: conversationKeys.all() })
    } catch (e) {
      toast.error(errorText(e, t('team.assignFailed')))
    } finally {
      setBusy(false)
    }
  }
  return (
    <label className="text-fg-muted ml-auto flex items-center gap-2 text-xs">
      {t('team.assignee')}
      <select
        className="border-border bg-bg rounded-md border px-2 py-1 text-xs"
        disabled={busy}
        onChange={(e) => void assign(e.target.value || null)}
        value={overview.assignee ?? ''}
      >
        <option value="">{t('team.unassigned')}</option>
        {overview.members.map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))}
      </select>
      {overview.assignee !== me && (
        <button
          className="text-accent hover:opacity-80"
          disabled={busy}
          onClick={() => void assign(me)}
          type="button"
        >
          {t('team.assignMe')}
        </button>
      )}
    </label>
  )
}

/** Someone else's reply in progress: read it, or take it over. */
function DraftLine({ c, me, overview }: { c: TeamConv; me: string; overview: WireSharedOverview }) {
  const t = useT()
  const [open, setOpen] = useState(false)
  const pointer = overview.draft
  const last = overview.handoffs[overview.handoffs.length - 1]
  const { data: draft } = useQuery({
    enabled: open && pointer !== null,
    queryKey: teamKeys.draft(c),
    queryFn: ({ signal }) => wireSharedDraft(c, signal),
  })
  if (!pointer || pointer.author === me) return null
  const takeOver = async () => {
    try {
      await wireTakeOverDraft(c)
      toast.success(t('team.draft.takenOver'))
      void queryClient.invalidateQueries({ queryKey: mailKeys.drafts() })
      void queryClient.invalidateQueries({ queryKey: teamKeys.overview(c) })
    } catch (e) {
      toast.error(errorText(e, t('team.draft.takeOverFailed')))
    }
  }
  return (
    <div className="space-y-1 text-xs">
      <div className="flex items-center gap-3">
        <span className="text-fg-secondary">{t('team.composing', { name: pointer.author })}</span>
        <button className="text-accent" onClick={() => setOpen(!open)} type="button">
          {t(readLabel(open))}
        </button>
        <button className="text-accent" onClick={() => void takeOver()} type="button">
          {t('team.draft.takeOver')}
        </button>
      </div>
      {last && (
        <p className="text-fg-muted">{t('team.draft.handoff', { from: last.from, to: last.to })}</p>
      )}
      {open && draft?.draft && (
        <pre className="border-border bg-bg max-h-48 overflow-auto rounded-md border p-2 whitespace-pre-wrap">
          {draft.draft.body}
        </pre>
      )}
    </div>
  )
}

function LastReply({ overview }: { overview: WireSharedOverview }) {
  const t = useT()
  if (overview.reply.seq === 0) return null
  return (
    <p className="text-fg-muted text-xs">
      {t('team.lastReply', { name: overview.reply.by, time: formatDate(overview.reply.at) })}
    </p>
  )
}

function PresenceLine({ me, overview }: { me: string; overview: WireSharedOverview }) {
  const t = useT()
  const others = overview.present.filter((p) => p.user !== me)
  const writing = others.filter((p) => p.state === 'composing')
  const viewing = others.filter((p) => p.state === 'viewing')
  return (
    <>
      {writing.map((p) => (
        <p className="text-warning text-xs" key={p.user} role="status">
          {t('team.composing', { name: p.user })}
        </p>
      ))}
      {viewing.length > 0 && (
        <p className="text-fg-muted text-xs">
          {t('team.viewing', { names: viewing.map((p) => p.user).join(', ') })}
        </p>
      )}
    </>
  )
}

function presenceState(composing: boolean) {
  if (composing) return 'composing' as const
  return 'viewing' as const
}

function readLabel(open: boolean) {
  if (open) return 'team.draft.hide' as const
  return 'team.draft.read' as const
}

function TeamPanelFor({ c }: { c: TeamConv }) {
  const t = useT()
  const me = useAtomValue(authAtom)?.address.toLowerCase() ?? ''
  const composing = useAtomValue(teamComposingAtom) === `${c.gid}:${c.conv}`
  usePresence(c, presenceState(composing))
  const { data } = useTeamOverview(c)
  if (!data) return null
  return (
    <section
      aria-label={t('team.panel')}
      className="border-border bg-bg-secondary/40 space-y-2 border-t px-4 py-3 text-sm"
    >
      <div className="flex flex-wrap items-center gap-2">
        <Users aria-hidden className="text-fg-muted h-4 w-4" />
        <span className="text-fg-secondary font-medium">{t('team.panel')}</span>
        <AssigneePicker c={c} me={me} overview={data} />
      </div>
      <PresenceLine me={me} overview={data} />
      <LastReply overview={data} />
      <DraftLine c={c} me={me} overview={data} />
      <TeamNotes c={c} me={me} />
    </section>
  )
}
