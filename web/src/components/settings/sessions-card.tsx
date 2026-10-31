import type { WireSession } from '@/wire/schemas/sessions'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'

import { queryClient } from '@/lib/query-client'
import { device } from '@/lib/user-agent'
import { wireEndOtherSessions, wireEndSession, wireListSessions } from '@/wire/endpoints/sessions'

import { btnSecondary, cardClass, SectionHeader } from './_shared'
import { errorText } from './rules-shared'

const sessionsKey = ['settings', 'sessions'] as const

/**
 * Where this account is signed in. A session that is not the reader's can
 * be signed out here; the reader is also mailed when a new device signs in.
 */
export function SessionsCard() {
  const { data } = useQuery({
    queryKey: sessionsKey,
    queryFn: ({ signal }) => wireListSessions(signal),
  })
  const sessions = data ?? []
  const refresh = () => queryClient.invalidateQueries({ queryKey: sessionsKey })
  const end = async (id: string) => {
    try {
      await wireEndSession(id)
      toast.success('Signed out')
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void refresh()
    }
  }
  const endOthers = async () => {
    try {
      const { ended } = await wireEndOtherSessions()
      toast.success(`Signed out ${ended} other sessions`)
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void refresh()
    }
  }
  return (
    <div className="space-y-3">
      <SectionHeader title="Signed-in sessions" />
      <ul className={`${cardClass} divide-border divide-y p-0`}>
        {sessions.map((s) => (
          <SessionRow key={s.id} onEnd={() => end(s.id)} session={s} />
        ))}
      </ul>
      {sessions.length > 1 && (
        <button className={btnSecondary} onClick={endOthers} type="button">
          Sign out of all other sessions
        </button>
      )}
    </div>
  )
}

function SessionRow({ onEnd, session }: { onEnd: () => void; session: WireSession }) {
  return (
    <li className="flex items-center justify-between gap-3 px-4 py-3 text-sm">
      <div className="min-w-0">
        <p className="truncate font-medium">
          {device(session.user_agent)}
          {session.current && <span className="text-success ml-2 text-xs">This device</span>}
        </p>
        <p className="text-fg-muted truncate text-xs">
          {session.ip || 'Unknown address'} · last used{' '}
          {new Date(session.last_seen * 1000).toLocaleString()}
        </p>
      </div>
      {!session.current && (
        <button className={btnSecondary} onClick={onEnd} type="button">
          Sign out
        </button>
      )}
    </li>
  )
}
