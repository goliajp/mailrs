import type { WireOutboundPaused } from '@/wire/schemas/admin'

import { Alert, Button } from '@goliapkg/gds'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { ConfirmAction } from '@/components/admin-page'
import { type Key, useT } from '@/i18n'
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
  const t = useT()
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
    <section aria-label={t('admin.paused.label')} className="mb-4 flex flex-col gap-2">
      {resume.data && (
        <Alert role="status" variant="success">
          {t('admin.paused.released', {
            discarded: resume.data.discarded,
            sent: resume.data.sent,
          })}
        </Alert>
      )}
      {items.map((p) => (
        <Alert key={p.address} role="alert" variant="warning">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0">
              <p className="font-medium break-all">
                {t('admin.paused.title', { address: p.address })}
              </p>
              <p className="text-sm">
                {t('admin.paused.detail', {
                  held: p.held,
                  reason: p.reason,
                  since: formatFullDate(p.since),
                })}
              </p>
            </div>
            <div className="flex gap-2">
              <Button
                onClick={() => setTarget({ account: p, choice: 'send' })}
                size="sm"
                variant="secondary"
              >
                {t('admin.paused.send')}
              </Button>
              <Button
                onClick={() => setTarget({ account: p, choice: 'discard' })}
                size="sm"
                variant="danger"
              >
                {t('admin.paused.discard')}
              </Button>
            </div>
          </div>
        </Alert>
      ))}
      {target && (
        <ConfirmAction
          confirmLabel={t(confirmLabel(target.choice, resume.isPending))}
          consequence={t(consequence(target.choice), { held: target.account.held })}
          onCancel={() => setTarget(null)}
          onConfirm={() => resume.mutate(target)}
          pending={resume.isPending}
          title={t('admin.paused.confirmTitle', { address: target.account.address })}
        />
      )}
    </section>
  )
}

function confirmLabel(choice: Choice, pending: boolean): Key {
  if (pending) return 'admin.paused.releasing'
  if (choice === 'send') return 'admin.paused.send'
  return 'admin.paused.discard'
}

function consequence(choice: Choice): Key {
  if (choice === 'send') return 'admin.paused.sendConsequence'
  return 'admin.paused.discardConsequence'
}
