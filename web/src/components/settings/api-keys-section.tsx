import type { AgentKey, CreatedAgentKey } from './_shared'
import type { ScopePreset } from './api-keys/scopes'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { KeyRound, Plus, SearchX } from 'lucide-react'
import { useMemo, useState } from 'react'

import { type Key, useT } from '@/i18n'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  wireCreateAgentKey,
  wireDeleteAgentKey,
  wireListAgentKeys,
} from '@/wire/endpoints/settings'

import { btnPrimary, btnSecondary, ConfirmDialog } from './_shared'
import { copyText } from './api-keys/copy-text'
import { CreateKeyForm } from './api-keys/create-key-form'
import { CreatedKeyPanel } from './api-keys/created-key-panel'
import { KeyTable } from './api-keys/key-table'
import { presetToScopes } from './api-keys/scopes'
import { clampPage, filterKeys, pageCount, pageSlice, sortKeys } from './api-keys/table-model'
import { TablePagination } from './api-keys/table-pagination'
import { TableToolbar } from './api-keys/table-toolbar'
import { useKeyTableParams } from './api-keys/use-key-table-params'

const EMPTY_KEYS: readonly AgentKey[] = []

type Translate = (key: Key, vars?: Record<string, number | string>) => string

export function ApiKeysSection() {
  const t = useT()
  const query = useQuery({
    queryKey: settingsKeys.agentKeys(),
    queryFn: () => wireListAgentKeys(),
  })
  const { params, setPage, setQuery, setSize, toggleSort } = useKeyTableParams()
  const [adding, setAdding] = useState(false)
  const [busy, setBusy] = useState(false)
  const [createdKey, setCreatedKey] = useState<CreatedAgentKey | null>(null)
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set())
  const [revokeTargets, setRevokeTargets] = useState<null | readonly AgentKey[]>(null)
  const now = useMemo(() => new Date(), [])

  const keys = query.data ?? EMPTY_KEYS
  const matched = useMemo(() => filterKeys(keys, params.query), [keys, params.query])
  const ordered = useMemo(
    () => sortKeys(matched, params.sort, params.dir),
    [matched, params.dir, params.sort]
  )
  const page = clampPage(params.page, ordered.length, params.size)
  const pageRows = pageSlice(ordered, page, params.size)
  // a selection can outlive the rows it names (revoked elsewhere, filtered
  // out); reconcile against the live set instead of trusting the state
  const liveSelection = useMemo(() => keys.filter((key) => selected.has(key.id)), [keys, selected])

  const invalidate = () => queryClient.invalidateQueries({ queryKey: settingsKeys.agentKeys() })

  const handleCreate = async (values: { name: string; preset: ScopePreset }) => {
    setBusy(true)
    try {
      const created = await wireCreateAgentKey({
        name: values.name,
        scopes: presetToScopes(values.preset),
      })
      toast.success(t('settings.apiKeys.created'))
      setCreatedKey(created)
      setAdding(false)
      void invalidate()
    } catch (e) {
      toast.error(errorMessage(e, t('settings.apiKeys.createFailed')))
    } finally {
      setBusy(false)
    }
  }

  const handleRevoke = async (targets: readonly AgentKey[]) => {
    setBusy(true)
    let revoked = 0
    for (const target of targets) {
      try {
        await wireDeleteAgentKey(target.id)
        revoked += 1
      } catch (e) {
        toast.error(errorMessage(e, t('settings.apiKeys.revokeFailed', { name: target.name })))
      }
    }
    if (revoked > 0) toast.success(revokedLabel(revoked, t))
    setRevokeTargets(null)
    setSelected(new Set())
    setBusy(false)
    void invalidate()
  }

  const toggleRow = (id: string, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (checked) next.add(id)
      else next.delete(id)
      return next
    })
  }

  const toggleAll = (checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev)
      for (const row of pageRows) {
        if (checked) next.add(row.id)
        else next.delete(row.id)
      }
      return next
    })
  }

  return (
    <div className="space-y-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold tracking-tight">{t('settings.apiKeys.title')}</h2>
          <p className="text-fg-muted mt-0.5 text-xs">{t('settings.apiKeys.help')}</p>
        </div>
        <button
          className={createButtonClass(adding)}
          disabled={adding}
          onClick={() => setAdding(true)}
        >
          <Plus aria-hidden className="h-4 w-4" />
          {t('settings.apiKeys.create')}
        </button>
      </header>

      {createdKey && <CreatedKeyPanel created={createdKey} onDismiss={() => setCreatedKey(null)} />}

      {adding && (
        <CreateKeyForm busy={busy} onCancel={() => setAdding(false)} onCreate={handleCreate} />
      )}

      {query.isError && <ErrorState error={query.error} onRetry={() => void query.refetch()} />}

      {query.isPending && <TableSkeleton />}

      {!query.isPending && !query.isError && keys.length === 0 && !adding && (
        <EmptyState
          description={t('settings.apiKeys.emptyHelp')}
          icon={<KeyRound aria-hidden className="h-8 w-8" />}
          title={t('settings.apiKeys.empty')}
        />
      )}

      {!query.isPending && !query.isError && keys.length > 0 && (
        <div className="space-y-3">
          <TableToolbar
            filtered={matched.length}
            onClearSelection={() => setSelected(new Set())}
            onCopyIds={() =>
              void copyText(
                liveSelection.map((k) => k.id).join('\n'),
                t('settings.apiKeys.copyIdsLabel')
              )
            }
            onCopyJson={() =>
              void copyText(
                JSON.stringify(liveSelection, null, 2),
                t('settings.apiKeys.copyJsonLabel')
              )
            }
            onQueryChange={setQuery}
            onRevokeSelected={() => setRevokeTargets(liveSelection)}
            onSizeChange={setSize}
            query={params.query}
            selectedCount={liveSelection.length}
            size={params.size}
            total={keys.length}
          />

          {matched.length === 0 && (
            <EmptyState
              description={t('settings.apiKeys.noMatchHelp', {
                count: keys.length,
                query: params.query,
              })}
              icon={<SearchX aria-hidden className="h-8 w-8" />}
              title={t('settings.apiKeys.noMatch')}
            />
          )}

          {matched.length > 0 && (
            <KeyTable
              now={now}
              onRevoke={(key) => setRevokeTargets([key])}
              onSort={toggleSort}
              onToggleAll={toggleAll}
              onToggleRow={toggleRow}
              rows={pageRows}
              selected={selected}
              sortDir={params.dir}
              sortField={params.sort}
            />
          )}

          {matched.length > 0 && (
            <TablePagination
              onPageChange={setPage}
              page={page}
              pages={pageCount(matched.length, params.size)}
              rangeEnd={(page - 1) * params.size + pageRows.length}
              rangeStart={(page - 1) * params.size + 1}
              total={matched.length}
            />
          )}
        </div>
      )}

      {revokeTargets !== null && revokeTargets.length > 0 && (
        <ConfirmDialog
          message={revokeMessage(revokeTargets, t)}
          onCancel={() => setRevokeTargets(null)}
          onConfirm={() => void handleRevoke(revokeTargets)}
        />
      )}
    </div>
  )
}

function createButtonClass(adding: boolean): string {
  const base = `${btnPrimary} inline-flex items-center gap-1.5`
  if (adding) return `${base} pointer-events-none`
  return base
}

function EmptyState({
  description,
  icon,
  title,
}: {
  description: string
  icon: React.ReactNode
  title: string
}) {
  return (
    <div className="border-border text-fg-muted flex flex-col items-center gap-2 rounded-lg border border-dashed px-6 py-10 text-center">
      {icon}
      <p className="text-fg text-sm font-medium">{title}</p>
      <p className="max-w-md text-xs">{description}</p>
    </div>
  )
}

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message
  return fallback
}

function ErrorState({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const t = useT()
  return (
    <div className="border-danger/40 bg-danger/5 rounded-lg border px-4 py-3" role="alert">
      <p className="text-sm font-medium">{t('settings.apiKeys.loadFailed')}</p>
      <p className="text-fg-secondary mt-1 text-xs">
        {errorMessage(error, t('settings.unknownError'))}
      </p>
      <button className={`${btnSecondary} mt-2`} onClick={onRetry} type="button">
        {t('common.retry')}
      </button>
    </div>
  )
}

function revokedLabel(count: number, t: Translate): string {
  if (count === 1) return t('settings.apiKeys.revokedOne')
  return t('settings.apiKeys.revokedMany', { count })
}

function revokeMessage(targets: readonly AgentKey[], t: Translate): string {
  if (targets.length === 1) {
    return t('settings.apiKeys.revokeOneConfirm', {
      name: targets[0].name,
      prefix: targets[0].prefix,
    })
  }
  return t('settings.apiKeys.revokeManyConfirm', { count: targets.length })
}

function TableSkeleton() {
  const t = useT()
  return (
    <div className="border-border overflow-hidden rounded-lg border" role="status">
      <div className="bg-bg-secondary h-9" />
      {[0, 1, 2, 3].map((row) => (
        <div className="border-border/60 flex items-center gap-3 border-t px-3 py-3" key={row}>
          <div className="bg-bg-secondary h-3 w-32 animate-pulse rounded" />
          <div className="bg-bg-secondary h-3 w-20 animate-pulse rounded" />
          <div className="bg-bg-secondary ml-auto h-3 w-24 animate-pulse rounded" />
        </div>
      ))}
      <span className="sr-only">{t('settings.apiKeys.loading')}</span>
    </div>
  )
}
