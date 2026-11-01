import type { ConfigEntry } from '@/components/admin-system-config/entry'
import type { ReactNode } from 'react'

import { useQuery } from '@tanstack/react-query'
import { Settings2 } from 'lucide-react'
import { useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminSection,
  ConfirmAction,
} from '@/components/admin-page'
import { ConfigField } from '@/components/admin-system-config/config-field'
import { resetVerb } from '@/components/admin-system-config/entry'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminObjectGet, adminPut } from '@/wire/endpoints/admin'

type GroupedEntries = Record<string, ConfigEntry[]>

type SystemConfigResponse = {
  entries?: ConfigEntry[]
  message?: string
  success: boolean
}

// the one setting that can change here first, keys nothing reads last
const GROUP_ORDER = ['smtp', 'imap', 'security', 'custom']

const GROUP_LABELS: Record<string, Key> = {
  custom: 'admin.config.group.custom',
  imap: 'admin.proto.imap',
  security: 'admin.config.group.security',
  smtp: 'admin.proto.smtp',
}

export function AdminSystemConfig() {
  const t = useT()
  const {
    data: response,
    error: queryError,
    isPending,
    refetch,
  } = useQuery({
    queryKey: adminKeys.systemConfig(),
    queryFn: ({ signal }) => adminObjectGet<SystemConfigResponse>('/admin/system-config', signal),
  })

  const entries = response?.entries ?? EMPTY_ENTRIES
  const error = loadError(queryError, response, t('admin.config.noSettingsAnswer'))

  const saveConfig = useAdminMutation({
    invalidateKey: adminKeys.systemConfig(),
    mutationFn: (vars: { key: string; value: string }) =>
      adminPut(`/admin/system-config/${encodeURIComponent(vars.key)}`, { value: vars.value }),
    successMsg: (vars) => t('admin.config.saved', { key: vars.key }),
  })

  const [resetTarget, setResetTarget] = useState<ConfigEntry | null>(null)
  const resetConfig = useAdminMutation({
    invalidateKey: adminKeys.systemConfig(),
    mutationFn: (entry: ConfigEntry) =>
      adminDelete(`/admin/system-config/${encodeURIComponent(entry.key)}`),
    successMsg: (entry) => t(resetDone(entry), { key: entry.key }),
  })

  const handleSave = (key: string, value: string) => {
    saveConfig.mutate({ key, value })
  }

  const handleReset = (entry: ConfigEntry) => {
    resetConfig.mutate(entry, { onSettled: () => setResetTarget(null) })
  }

  const grouped: GroupedEntries = {}
  for (const entry of entries) {
    const g = entry.group
    if (!grouped[g]) grouped[g] = []
    grouped[g].push(entry)
  }
  const sortedGroups = Object.keys(grouped).sort((a, b) => groupRank(a) - groupRank(b))

  const renderBody = () => {
    if (isPending) return <PanelGroupSkeleton />
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          what={t('admin.config.listName')}
        />
      )
    }
    if (entries.length === 0) {
      return (
        <AdminEmptyState
          description={t('admin.config.emptyHelp')}
          icon={<Settings2 className="h-10 w-10" />}
          title={t('admin.config.empty')}
        />
      )
    }
    return (
      <div className="flex flex-col gap-4">
        {sortedGroups.map((group) => (
          <GroupCard group={group} key={group}>
            {grouped[group].map((entry) => (
              <ConfigField
                entry={entry}
                key={entry.key}
                onReset={setResetTarget}
                onSave={handleSave}
                saving={saveConfig.isPending && saveConfig.variables?.key === entry.key}
              />
            ))}
          </GroupCard>
        ))}
      </div>
    )
  }

  return (
    <AdminPageShell
      subtitle={t('admin.config.subtitle')}
      title={t('admin.audit.group.systemConfig')}
    >
      {renderBody()}
      {resetTarget && (
        <ConfirmAction
          confirmLabel={t(confirmLabel(resetConfig.isPending, resetTarget))}
          consequence={resetConsequence(resetTarget, t)}
          onCancel={() => setResetTarget(null)}
          onConfirm={() => handleReset(resetTarget)}
          pending={resetConfig.isPending}
          title={t(confirmTitle(resetTarget), { key: resetTarget.key })}
        />
      )}
    </AdminPageShell>
  )
}

const EMPTY_ENTRIES: ConfigEntry[] = []

function confirmLabel(pending: boolean, entry: ConfigEntry): Key {
  if (!pending) return resetVerb(entry)
  if (entry.editable) return 'admin.config.resetting'
  return 'admin.removing'
}

function confirmTitle(entry: ConfigEntry): Key {
  if (entry.editable) return 'admin.config.resetTitle'
  return 'admin.config.removeTitle'
}

function GroupCard({ children, group }: { children: ReactNode; group: string }) {
  const t = useT()
  return <AdminSection title={groupLabel(group, t)}>{children}</AdminSection>
}

function groupLabel(group: string, t: ReturnType<typeof useT>): string {
  const key = GROUP_LABELS[group]
  if (key) return t(key)
  return group
}

function loadError(
  queryError: unknown,
  response: SystemConfigResponse | undefined,
  noAnswer: string
): Error | null {
  if (queryError instanceof Error) return queryError
  if (response && !response.success) {
    return new Error(response.message ?? noAnswer)
  }
  return null
}

function PanelGroupSkeleton() {
  return (
    <div aria-busy="true" className="flex flex-col gap-4">
      {[0, 1].map((i) => (
        <div className="border-border overflow-hidden rounded-lg border" key={i}>
          <div className="border-border bg-bg-secondary border-b px-4 py-2.5">
            <span className="bg-fg-muted/30 block h-4 w-24 animate-pulse rounded" />
          </div>
          {[0, 1, 2].map((j) => (
            <div className="border-border space-y-2 border-b px-4 py-4 last:border-0" key={j}>
              <span className="bg-fg-muted/30 block h-4 w-1/3 animate-pulse rounded" />
              <span className="bg-fg-muted/20 block h-3 w-2/3 animate-pulse rounded" />
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

// who picks up a reset, for the keys a running process reads
const READERS: Record<string, Key> = {
  max_message_size_bytes: 'admin.config.reader.maxSize',
}

function groupRank(group: string): number {
  const i = GROUP_ORDER.indexOf(group)
  if (i === -1) return GROUP_ORDER.length
  return i
}

// what DELETE /api/admin/system-config/{key} does: drop the stored value and
// its saved-by line, and write a `system_config.reset` audit row
function resetConsequence(entry: ConfigEntry, t: ReturnType<typeof useT>): string {
  const logged = t('admin.config.logged')
  if (entry.editable) {
    const parts = [
      t('admin.config.resetValue', {
        default: entry.default_value ?? '',
        value: entry.value ?? '',
      }),
    ]
    if (READERS[entry.key]) parts.push(t(READERS[entry.key]))
    parts.push(logged)
    return parts.join(t('admin.config.sentenceJoin'))
  }
  return t('admin.config.removeUnread', { key: entry.key, logged })
}

function resetDone(entry: ConfigEntry): Key {
  if (entry.editable) return 'admin.config.resetDone'
  return 'admin.config.removed'
}
