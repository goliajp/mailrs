import type { WireAccountAgentKey } from '@/wire/schemas/admin'

import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { MobileModal } from '@/components/mobile-modal'
import { btnDanger, btnSecondary, inputClass } from '@/components/settings/_shared'
import { formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { wireAccountAgentKeys, wireCompromiseReset } from '@/wire/endpoints/admin'

type Props = {
  address: string
  onClose: () => void
}

/**
 * A new password for an account someone else got into: its sessions end
 * and every agent key is revoked. The keys are listed first so the
 * administrator knows which services stop; they are not chosen.
 */
export function CompromiseResetDialog({ address, onClose }: Props) {
  const [password, setPassword] = useState('')
  // keys the server sent back when the list changed under the dialog
  const [resent, setResent] = useState<null | WireAccountAgentKey[]>(null)
  const listed = useQuery({
    queryKey: adminKeys.accountAgentKeys(address),
    queryFn: () => wireAccountAgentKeys(address),
  })
  const reset = useMutation({
    mutationFn: (keyIds: number[]) => wireCompromiseReset(address, password, keyIds),
    onSuccess: (outcome) => {
      if (outcome.kind === 'key-list-changed') setResent(outcome.keys)
    },
  })

  const keys = resent ?? listed.data?.items ?? []
  const since = listed.data?.last_use_recorded_since ?? ''
  const outcome = reset.data

  return (
    <MobileModal onClose={onClose} open>
      <div className="bg-surface flex w-full max-w-lg flex-col gap-4 rounded-lg p-6 shadow-lg">
        <h2 className="text-base font-semibold break-all">Reset {address} after a compromise</h2>
        <p className="text-fg-secondary text-sm">
          The account gets a new password, everyone signed in to it is signed out, and every agent
          key below is revoked. Services that use these keys stop until they are given new ones.
        </p>

        {listed.error && <p className="text-danger text-sm">{listed.error.message}</p>}
        {resent && (
          <p className="text-warning text-sm" role="status">
            The account&apos;s keys changed since this list was shown. Check the list again.
          </p>
        )}
        <KeyList keys={keys} loading={listed.isPending} since={since} />

        {outcome?.kind === 'done' && (
          <p className="text-success text-sm" role="status">
            Password set. {outcome.sessionsRevoked} sessions ended, {outcome.agentKeysRevoked} agent
            keys revoked.
          </p>
        )}
        {outcome?.kind === 'refused' && (
          <p className="text-danger text-sm" role="alert">
            {outcome.message}
          </p>
        )}
        {reset.error && (
          <p className="text-danger text-sm" role="alert">
            {reset.error.message}
          </p>
        )}

        {outcome?.kind !== 'done' && (
          <label className="flex flex-col gap-1">
            <span className="text-fg-secondary text-xs font-medium">New password</span>
            <input
              autoComplete="new-password"
              className={inputClass}
              onChange={(e) => setPassword(e.target.value)}
              type="password"
              value={password}
            />
          </label>
        )}

        <div className="flex justify-end gap-2">
          <button className={btnSecondary} onClick={onClose} type="button">
            {closeLabel(outcome?.kind === 'done')}
          </button>
          {outcome?.kind !== 'done' && (
            <button
              className={btnDanger}
              disabled={!password || listed.isPending || reset.isPending || !!listed.error}
              onClick={() => reset.mutate(keys.map((k) => k.id))}
              type="button"
            >
              Reset and revoke {keys.length} keys
            </button>
          )}
        </div>
      </div>
    </MobileModal>
  )
}

function closeLabel(done: boolean): string {
  if (done) return 'Close'
  return 'Cancel'
}

function dateOrDash(ts: null | number): string {
  if (ts === null) return '—'
  return formatFullDate(ts)
}

function KeyList({
  keys,
  loading,
  since,
}: {
  keys: WireAccountAgentKey[]
  loading: boolean
  since: string
}) {
  if (loading) return <p className="text-fg-muted text-sm">Loading keys…</p>
  if (keys.length === 0)
    return <p className="text-fg-muted text-sm">The account has no agent keys.</p>
  return (
    <table className="w-full text-left text-xs">
      <thead className="text-fg-muted">
        <tr>
          <th className="py-1 font-medium">Key</th>
          <th className="py-1 font-medium">Created</th>
          <th className="py-1 font-medium">Last used</th>
        </tr>
      </thead>
      <tbody>
        {keys.map((k) => (
          <tr className="border-border border-t" key={k.id}>
            <td className="py-1.5">
              <span className="font-medium">{k.name || 'unnamed'}</span>{' '}
              <span className="text-fg-muted font-mono">{k.prefix}…</span>
            </td>
            <td className="py-1.5">{dateOrDash(k.created_at)}</td>
            <td className="py-1.5">{lastUsed(k.last_used_at, since)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

/** Recorded to the hour; a key with nothing recorded was not used since recording began. */
function lastUsed(ts: null | number, since: string): string {
  if (ts === null) return `Not recorded (recording began ${since})`
  return `${formatFullDate(ts)} (to the hour)`
}
