import type { WireOutboundPaused } from '@/wire/schemas/admin'

import { Alert, Button } from '@goliapkg/gds'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { ConfirmAction } from '@/components/admin-page'
import { formatFullDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { adminKeys } from '@/lib/query-keys'
import { wireOutboundPaused, wireResumeOutbound } from '@/wire/endpoints/admin'

type Choice = 'discard' | 'send'
type Paused = WireOutboundPaused['items'][number]

/**
 * Accounts whose outbound mail is paused: each sent far past its hourly
 * limit or kept failing, and its mail is held. Releasing one sends or
 * discards what it held. Nothing shows while no account is paused.
 */
export function PausedSenders() {
  const [target, setTarget] = useState<null | { account: Paused; choice: Choice }>(null)
  const paused = useQuery({
    queryKey: adminKeys.outboundPaused(),
    refetchInterval: 30_000,
    queryFn: ({ signal }) => wireOutboundPaused(signal),
  })
  const resume = useMutation({
    mutationFn: (t: { account: Paused; choice: Choice }) =>
      wireResumeOutbound(t.account.address, t.choice),
    onSettled: () => {
      setTarget(null)
      queryClient.invalidateQueries({ queryKey: adminKeys.outboundPaused() })
      queryClient.invalidateQueries({ queryKey: adminKeys.queues() })
    },
  })

  const items = paused.data?.items ?? []
  if (items.length === 0 && !resume.data) return null

  return (
    <section aria-label="Paused senders" className="mb-4 flex flex-col gap-2">
      {resume.data && (
        <Alert role="status" variant="success">
          Released: {resume.data.sent} sent, {resume.data.discarded} discarded.
        </Alert>
      )}
      {items.map((p) => (
        <Alert key={p.address} role="alert" variant="warning">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0">
              <p className="font-medium break-all">Outbound mail from {p.address} is paused</p>
              <p className="text-sm">
                {p.reason}. Since {formatFullDate(p.since)}; {p.held} messages held.
              </p>
            </div>
            <div className="flex gap-2">
              <Button
                onClick={() => setTarget({ account: p, choice: 'send' })}
                size="sm"
                variant="secondary"
              >
                Release and send
              </Button>
              <Button
                onClick={() => setTarget({ account: p, choice: 'discard' })}
                size="sm"
                variant="danger"
              >
                Release and discard
              </Button>
            </div>
          </div>
        </Alert>
      ))}
      {target && (
        <ConfirmAction
          confirmLabel={confirmLabel(target.choice, resume.isPending)}
          consequence={consequence(target)}
          onCancel={() => setTarget(null)}
          onConfirm={() => resume.mutate(target)}
          pending={resume.isPending}
          title={`Release ${target.account.address}?`}
        />
      )}
    </section>
  )
}

function confirmLabel(choice: Choice, pending: boolean): string {
  if (pending) return 'Releasing...'
  if (choice === 'send') return 'Release and send'
  return 'Release and discard'
}

function consequence(t: { account: Paused; choice: Choice }): string {
  if (t.choice === 'send') {
    return `The ${t.account.held} held messages go out, and the account sends again. Check first that the mail it was sending is its own.`
  }
  return `The ${t.account.held} held messages are deleted unsent, and the account sends again.`
}
