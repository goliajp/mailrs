import type { ReactNode } from 'react'

import { useQuery } from '@tanstack/react-query'
import { RotateCcw, Save, Settings2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { AdminEmptyState, AdminErrorState, AdminPageShell } from '@/components/admin-page'
import { ReadOnlyField } from '@/components/admin-system-config/read-only-field'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { adminKeys } from '@/lib/query-keys'
import { adminDelete, adminObjectGet, adminPut } from '@/wire/endpoints/admin'

// Backend: crates/webapi/src/handlers/system_config.rs — fn get_system_config
type ConfigEntry = {
  default_value: null | string
  description: string
  /** false for settings read once at process start, and for keys nothing reads */
  editable: boolean
  /** where a read-only setting is set */
  env_var?: string
  group: string
  key: string
  note?: null | string
  source: string
  /** unix seconds; only on a value saved from this page or the MCP tool */
  updated_at?: number
  updated_by?: string
  /** null when webapi cannot see the value */
  value: null | string
}

type GroupedEntries = Record<string, ConfigEntry[]>

type SystemConfigResponse = {
  entries?: ConfigEntry[]
  message?: string
  success: boolean
}

const GROUP_LABELS: Record<string, string> = {
  custom: 'Custom',
  imap: 'IMAP',
  security: 'Security',
  smtp: 'SMTP',
}

const SOURCE_STYLES: Record<string, string> = {
  database: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300',
  default: 'bg-gray-100 text-gray-600 dark:bg-gray-800 dark:text-gray-400',
  env: 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/40 dark:text-yellow-300',
  unknown: 'bg-gray-100 text-gray-600 dark:bg-gray-800 dark:text-gray-400',
}

const SOURCE_LABELS: Record<string, string> = {
  database: 'Database',
  default: 'Default',
  env: 'Environment',
  unknown: 'Not visible here',
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

  const resetConfig = useAdminMutation({
    invalidateKey: adminKeys.systemConfig(),
    mutationFn: (key: string) => adminDelete(`/admin/system-config/${encodeURIComponent(key)}`),
    successMsg: (key) => `"${key}" reset to default`,
  })

  const handleSave = (key: string, value: string) => {
    saveConfig.mutate({ key, value })
  }

  const handleReset = (key: string) => {
    resetConfig.mutate(key)
  }

  const grouped: GroupedEntries = {}
  for (const entry of entries) {
    const g = entry.group
    if (!grouped[g]) grouped[g] = []
    grouped[g].push(entry)
  }
  const sortedGroups = Object.keys(grouped).sort()

  const renderBody = () => {
    if (isPending) return <PanelGroupSkeleton />
    if (error) return <AdminErrorState error={error} onRetry={() => refetch()} />
    if (entries.length === 0) {
      return (
        <AdminEmptyState
          description="The server returned no settings"
          icon={<Settings2 className="h-10 w-10" />}
          title="No configuration entries"
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
                onReset={handleReset}
                onSave={handleSave}
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
      title="System Configuration"
    >
      {renderBody()}
    </AdminPageShell>
  )
}

const EMPTY_ENTRIES: ConfigEntry[] = []

function ConfigField({
  entry,
  onReset,
  onSave,
}: {
  entry: ConfigEntry
  onReset: (key: string) => void
  onSave: (key: string, value: string) => void
}) {
  const [localValue, setLocalValue] = useState(entry.value ?? '')
  const [saving, setSaving] = useState(false)
  const [resetting, setResetting] = useState(false)

  // sync local value when entry changes from parent
  useEffect(() => {
    setLocalValue(entry.value ?? '')
  }, [entry.value])

  const dirty = localValue !== (entry.value ?? '')
  const saved = savedBy(entry)

  const handleSave = async () => {
    setSaving(true)
    try {
      await onSave(entry.key, localValue)
    } finally {
      setSaving(false)
    }
  }

  const handleReset = async () => {
    setResetting(true)
    try {
      await onReset(entry.key)
    } finally {
      setResetting(false)
    }
  }

  const resetButton = entry.source === 'database' && (
    <button
      className="text-fg-secondary hover:bg-bg-secondary inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs transition-colors disabled:opacity-50"
      disabled={resetting}
      onClick={handleReset}
    >
      <RotateCcw className="h-3 w-3" />
      {resetting && 'Resetting...'}
      {!resetting && 'Reset to default'}
    </button>
  )

  if (!entry.editable) {
    return (
      <ReadOnlyField
        badge={<SourceBadge source={entry.source} />}
        description={entry.description}
        envVar={entry.env_var}
        label={entry.key}
        note={entry.note}
        reset={resetButton}
        value={entry.value}
      />
    )
  }

  return (
    <div className="border-border border-b px-4 py-4 last:border-0">
      <div className="mb-1.5 flex flex-wrap items-center gap-2">
        <code className="text-fg text-sm font-medium">{entry.key}</code>
        <SourceBadge source={entry.source} />
        {saved && <span className="text-fg-muted text-xs">{saved}</span>}
      </div>
      {entry.description && <p className="text-fg-secondary mb-2 text-xs">{entry.description}</p>}
      <div className="flex flex-wrap items-center gap-2">
        <input
          aria-label={entry.key}
          className="border-border bg-bg-secondary w-full max-w-md rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setLocalValue(e.target.value)}
          type="text"
          value={localValue}
        />
        <button
          className="bg-fg text-bg inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs font-medium transition-colors hover:opacity-90 disabled:opacity-50"
          disabled={!dirty || saving}
          onClick={handleSave}
        >
          <Save className="h-3 w-3" />
          {saving && 'Saving...'}
          {!saving && 'Save'}
        </button>
        {resetButton}
      </div>
    </div>
  )
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
    return new Error(response.message ?? 'Failed to load configuration')
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

function savedBy(entry: ConfigEntry): null | string {
  if (entry.updated_at === undefined) return null
  const on = new Date(entry.updated_at * 1000).toLocaleString()
  if (entry.updated_by) return `Saved by ${entry.updated_by} on ${on}`
  return `Saved on ${on}`
}

function SourceBadge({ source }: { source: string }) {
  const style = SOURCE_STYLES[source] ?? SOURCE_STYLES.default
  const label = SOURCE_LABELS[source] ?? source
  return <span className={`rounded px-1.5 py-0.5 text-xs font-medium ${style}`}>{label}</span>
}
