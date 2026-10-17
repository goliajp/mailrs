import type { ConfigEntry } from '@/components/admin-system-config/entry'
import type { ReactNode } from 'react'

import { useQuery } from '@tanstack/react-query'
import { Settings2 } from 'lucide-react'
import { useState } from 'react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  ConfirmAction,
} from '@/components/admin-page'
import { ConfigField } from '@/components/admin-system-config/config-field'
import { resetVerb } from '@/components/admin-system-config/entry'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
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

const GROUP_LABELS: Record<string, string> = {
  custom: 'Stored keys nothing reads',
  imap: 'IMAP',
  security: 'Security',
  smtp: 'SMTP',
}

export function AdminSystemConfig() {
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
  const error = loadError(queryError, response)

  const saveConfig = useAdminMutation({
    invalidateKey: adminKeys.systemConfig(),
    mutationFn: (vars: { key: string; value: string }) =>
      adminPut(`/admin/system-config/${encodeURIComponent(vars.key)}`, { value: vars.value }),
    successMsg: (vars) => `"${vars.key}" saved`,
  })

  const [resetTarget, setResetTarget] = useState<ConfigEntry | null>(null)
  const resetConfig = useAdminMutation({
    invalidateKey: adminKeys.systemConfig(),
    mutationFn: (entry: ConfigEntry) =>
      adminDelete(`/admin/system-config/${encodeURIComponent(entry.key)}`),
    successMsg: (entry) => resetDone(entry),
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
      return <AdminErrorState error={error} onRetry={() => refetch()} what="the server settings" />
    }
    if (entries.length === 0) {
      return (
        <AdminEmptyState
          description="Settings this server's processes read are listed here, with where each one is set"
          icon={<Settings2 className="h-10 w-10" />}
          title="No settings yet"
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
      subtitle="Ports, hostname and TLS are set in compose and read when each process starts. Local domains are managed on the Domains page"
      title="System config"
    >
      {renderBody()}
      {resetTarget && (
        <ConfirmAction
          confirmLabel={confirmLabel(resetConfig.isPending, resetTarget)}
          consequence={resetConsequence(resetTarget)}
          onCancel={() => setResetTarget(null)}
          onConfirm={() => handleReset(resetTarget)}
          pending={resetConfig.isPending}
          title={confirmTitle(resetTarget)}
        />
      )}
    </AdminPageShell>
  )
}

const EMPTY_ENTRIES: ConfigEntry[] = []

function confirmLabel(pending: boolean, entry: ConfigEntry): string {
  if (!pending) return resetVerb(entry)
  if (entry.editable) return 'Resetting...'
  return 'Removing...'
}

function confirmTitle(entry: ConfigEntry): string {
  if (entry.editable) return `Reset ${entry.key}?`
  return `Remove ${entry.key}?`
}

function GroupCard({ children, group }: { children: ReactNode; group: string }) {
  const label = GROUP_LABELS[group] ?? group
  return (
    <div className="border-border overflow-hidden rounded-lg border">
      <div className="border-border bg-bg-secondary border-b px-4 py-2.5">
        <h3 className="text-sm font-semibold">{label}</h3>
      </div>
      {children}
    </div>
  )
}

function loadError(queryError: unknown, response: SystemConfigResponse | undefined): Error | null {
  if (queryError instanceof Error) return queryError
  if (response && !response.success) {
    return new Error(response.message ?? 'The server answered without its settings')
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
const READERS: Record<string, string> = {
  max_message_size_bytes: 'The receiver uses it for new connections within a minute',
}

function groupRank(group: string): number {
  const i = GROUP_ORDER.indexOf(group)
  if (i === -1) return GROUP_ORDER.length
  return i
}

// what DELETE /api/admin/system-config/{key} does: drop the stored value and
// its saved-by line, and write a `system_config.reset` audit row
function resetConsequence(entry: ConfigEntry): string {
  const logged = 'The change is recorded in the audit log'
  if (entry.editable) {
    const parts = [
      `${entry.value ?? ''} is removed and the built-in ${entry.default_value ?? ''} applies again`,
    ]
    if (READERS[entry.key]) parts.push(READERS[entry.key])
    parts.push(logged)
    return parts.join('. ')
  }
  return `No process reads ${entry.key}, so removing the stored value changes nothing on the server. ${logged}`
}

function resetDone(entry: ConfigEntry): string {
  if (entry.editable) return `"${entry.key}" reset to default`
  return `"${entry.key}" removed`
}
