import type { WireRecallBatch } from '@/wire/schemas/rejudge'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
  ConfirmAction,
} from '@/components/admin-page'
import { type Key, type Translate, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { recallBatchCount, recallBatchState, type RecallBatchState } from '@/lib/recall-batch'
import {
  fetchRecallBatches,
  wireConfirmRecallBatch,
  wireDiscardRecallBatch,
  wireReleaseRecallBatch,
} from '@/wire/endpoints/rejudge'
import { isWireError } from '@/wire/errors'

const EMPTY: WireRecallBatch[] = []

type Asking = { batch: WireRecallBatch; decision: Weighed }

type Decision = 'confirm' | 'discard' | 'release'

/** The two decisions on a waiting round, which a second dialog asks about first. */
type Weighed = Exclude<Decision, 'release'>

const STATE_LABEL: Record<RecallBatchState, Key> = {
  discarded: 'admin.recall.state.discarded',
  held: 'admin.recall.state.held',
  pending: 'admin.recall.state.pending',
  released: 'admin.recall.state.released',
}

const BUTTON: Record<Decision, Key> = {
  confirm: 'admin.recall.confirm',
  discard: 'admin.recall.discard',
  release: 'admin.recall.release',
}

const DONE: Record<Decision, Key> = {
  confirm: 'admin.recall.confirmed',
  discard: 'admin.recall.discarded',
  release: 'admin.recall.released',
}

const ASK: Record<Weighed, { consequence: Key; title: Key }> = {
  confirm: { consequence: 'admin.recall.ask.confirm', title: 'admin.recall.ask.confirmTitle' },
  discard: { consequence: 'admin.recall.ask.discard', title: 'admin.recall.ask.discardTitle' },
}

const SEND: Record<Decision, (observation: string) => Promise<number>> = {
  confirm: wireConfirmRecallBatch,
  discard: wireDiscardRecallBatch,
  release: wireReleaseRecallBatch,
}

const HEADERS: Key[] = [
  'admin.recall.batch',
  'admin.recall.state',
  'admin.recall.count',
  'admin.recall.rules',
  'admin.recall.at',
  'admin.recall.decidedAt',
]

/**
 * The batches re-judging took back into Review, one per round, and the
 * three decisions a super administrator makes on them: hold or drop a
 * round that waits over the cap, and put a held batch back whole. Holding
 * or dropping a round is asked about first; each write asks for the
 * password again when the server wants it.
 */
export function AdminRecallBatches() {
  const t = useT()
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: adminKeys.recallBatches(),
    queryFn: ({ signal }) => fetchRecallBatches(signal),
  })
  const items = data ?? EMPTY
  const [asking, setAsking] = useState<Asking | null>(null)
  const reload = () => void refetch()
  const decide = async (decision: Decision, observation: string) => {
    try {
      const count = await SEND[decision](observation)
      toast.success(t(DONE[decision], { count }))
    } catch (e) {
      toast.error(failureText(e, t))
    } finally {
      reload()
    }
  }
  const choose = (decision: Decision, batch: WireRecallBatch) => {
    if (decision === 'release') {
      void decide(decision, batch.observation)
      return
    }
    setAsking({ batch, decision })
  }

  const columns: Column<WireRecallBatch>[] = [
    { header: t('admin.recall.batch'), key: 'batch', cell: (b) => b.observation },
    {
      header: t('admin.recall.state'),
      key: 'state',
      cell: (b) => t(STATE_LABEL[recallBatchState(b)]),
    },
    {
      align: 'end',
      header: t('admin.recall.count'),
      key: 'count',
      cell: (b) => recallBatchCount(b),
    },
    { header: t('admin.recall.rules'), key: 'rules', cell: (b) => <Rules rules={b.rules} /> },
    { align: 'end', header: t('admin.recall.at'), key: 'at', cell: (b) => formatFullDate(b.at) },
    {
      align: 'end',
      header: t('admin.recall.decidedAt'),
      key: 'decided',
      cell: (b) => decidedAt(b),
    },
    {
      header: t('admin.recall.actions'),
      hideHeader: true,
      key: 'actions',
      cell: (b) => (
        <Actions decisions={decisionsFor(recallBatchState(b))} onDecide={(d) => choose(d, b)} />
      ),
    },
  ]

  return (
    <AdminPageShell subtitle={t('admin.recall.description')} title={t('admin.recall.title')}>
      {isPending && <AdminTableSkeleton cols={6} headers={HEADERS.map((h) => t(h))} rows={4} />}
      {error && (
        <AdminErrorState
          error={error}
          onRetry={reload}
          retryDisabled={isFetching}
          what={t('admin.recall.title')}
        />
      )}
      {!isPending && !error && items.length === 0 && (
        <AdminEmptyState title={t('admin.recall.empty')} />
      )}
      {items.length > 0 && (
        <AdminTable
          columns={columns}
          label={t('admin.recall.title')}
          rowKey={(b) => b.observation}
          rows={items}
        />
      )}
      {asking && (
        <ConfirmAction
          confirmLabel={t(BUTTON[asking.decision])}
          consequence={t(ASK[asking.decision].consequence, {
            count: recallBatchCount(asking.batch),
          })}
          onCancel={() => setAsking(null)}
          onConfirm={() => {
            setAsking(null)
            void decide(asking.decision, asking.batch.observation)
          }}
          title={t(ASK[asking.decision].title)}
        />
      )}
    </AdminPageShell>
  )
}

function Actions({
  decisions,
  onDecide,
}: {
  decisions: Decision[]
  onDecide: (d: Decision) => void
}) {
  const t = useT()
  if (decisions.length === 0) return null
  return (
    <div className="flex justify-end gap-2">
      {decisions.map((d) => (
        <button
          className="border-border hover:bg-bg-secondary rounded border px-2 py-1 text-xs whitespace-nowrap"
          key={d}
          onClick={() => onDecide(d)}
          type="button"
        >
          {t(BUTTON[d])}
        </button>
      ))}
    </div>
  )
}

/** When the batch was put back, discarded or confirmed; one that held on its own was never decided. */
function decidedAt(b: WireRecallBatch): string {
  if (b.released_at > 0) return formatFullDate(b.released_at)
  if (b.discarded_at > 0) return formatFullDate(b.discarded_at)
  if (b.confirmed_at > 0) return formatFullDate(b.confirmed_at)
  return '—'
}

function decisionsFor(state: RecallBatchState): Decision[] {
  switch (state) {
    case 'discarded':
    case 'released':
      return []
    case 'held':
      return ['release']
    case 'pending':
      return ['confirm', 'discard']
  }
}

function failureText(e: unknown, t: Translate): string {
  if (!isWireError(e)) return t('admin.recall.failed')
  if (e.detail.kind === 'unconfirmed') return t('admin.recall.unconfirmed')
  if (e.detail.kind === 'server' && e.detail.status === 409) return t('admin.recall.conflict')
  return t('admin.recall.failed')
}

function Rules({ rules }: { rules: string[] }) {
  if (rules.length === 0) return <span className="text-fg-muted">—</span>
  return (
    <ul className="text-xs">
      {rules.map((r) => (
        <li key={r}>{r}</li>
      ))}
    </ul>
  )
}
