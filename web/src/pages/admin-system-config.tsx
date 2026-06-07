import type { ReactNode } from 'react'

import { useQuery } from '@tanstack/react-query'
import { RotateCcw, Save, Settings2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { AdminEmptyState, AdminErrorState, AdminPageShell } from '@/components/admin-page'
import { useAdminMutation } from '@/hooks/use-admin-mutations'
import { deleteJson, fetchJson, putJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

type ConfigEntry = {
  description: string
  group: string
  key: string
  source: string
  updated_at: null | string
  updated_by: null | string
  value: string
  value_type: string
}

type GroupedEntries = Record<string, ConfigEntry[]>

const GROUP_LABELS: Record<string, string> = {
  ai: 'AI',
  antispam: 'Anti-Spam',
  security: 'Security',
  webhook: 'Webhook',
}

const SOURCE_STYLES: Record<string, string> = {
  database: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300',
  default: 'bg-gray-100 text-gray-600 dark:bg-gray-800 dark:text-gray-400',
  env: 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/40 dark:text-yellow-300',
}

const SOURCE_LABELS: Record<string, string> = {
  database: 'Database',
  default: 'Default',
  env: 'Environment',
}

export function AdminSystemConfig() {
  const {
    data: response,
    error: queryError,
    isPending,
    refetch,
  } = useQuery({
    queryKey: adminKeys.systemConfig(),
    queryFn: ({ signal }) =>
      fetchJson<{
        entries?: ConfigEntry[]
        message?: string
        success: boolean
      }>('/admin/system-config', signal),
  })

  const entries: ConfigEntry[] = response && response.success ? (response.entries ?? []) : []
  const error: Error | null =
    queryError instanceof Error
      ? queryError
      : response && !response.success
        ? new Error(response.message ?? 'Failed to load configuration')
        : null

  const saveConfig = useAdminMutation({
    invalidateKey: adminKeys.systemConfig(),
    mutationFn: (vars: { key: string; value: string }) =>
      putJson(`/admin/system-config/${encodeURIComponent(vars.key)}`, { value: vars.value }),
    successMsg: (vars) => `"${vars.key}" updated`,
  })

  const resetConfig = useAdminMutation({
    invalidateKey: adminKeys.systemConfig(),
    mutationFn: (key: string) => deleteJson(`/admin/system-config/${encodeURIComponent(key)}`),
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

  return (
    <AdminPageShell title="System Configuration">
      <p className="text-fg-secondary -mt-4 mb-6 text-sm">
        Runtime server settings. Changes to database-sourced values take effect immediately.
      </p>

      {isPending ? (
        <PanelGroupSkeleton />
      ) : error ? (
        <AdminErrorState error={error} onRetry={() => refetch()} />
      ) : entries.length === 0 ? (
        <AdminEmptyState
          description="The server didn't return any configurable settings."
          icon={<Settings2 className="h-10 w-10" />}
          title="No configuration entries"
        />
      ) : (
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
      )}
    </AdminPageShell>
  )
}

// render the appropriate form control based on value_type
function ConfigControl({
  onChange,
  value,
  valueType,
}: {
  onChange: (v: string) => void
  value: string
  valueType: string
}) {
  if (valueType === 'bool') {
    const checked = value === 'true'
    return (
      <button
        aria-checked={checked}
        aria-label="Toggle value"
        className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
          checked ? 'bg-accent' : 'bg-gray-300 dark:bg-gray-600'
        }`}
        onClick={() => onChange(checked ? 'false' : 'true')}
        role="switch"
        type="button"
      >
        <span
          className={`inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
            checked ? 'translate-x-6' : 'translate-x-1'
          }`}
        />
      </button>
    )
  }

  if (valueType === 'f64') {
    return (
      <input
        className="border-border bg-bg-secondary w-full max-w-xs rounded-md border px-3 py-1.5 text-sm"
        onChange={(e) => onChange(e.target.value)}
        step="any"
        type="number"
        value={value}
      />
    )
  }

  if (valueType.startsWith('enum:')) {
    const options = valueType.slice(5).split(',')
    return (
      <select
        className="border-border bg-bg-secondary rounded-md border px-3 py-1.5 text-sm"
        onChange={(e) => onChange(e.target.value)}
        value={value}
      >
        {options.map((opt) => (
          <option key={opt} value={opt}>
            {opt}
          </option>
        ))}
      </select>
    )
  }

  // default: string input
  return (
    <input
      className="border-border bg-bg-secondary w-full max-w-md rounded-md border px-3 py-1.5 text-sm"
      onChange={(e) => onChange(e.target.value)}
      type="text"
      value={value}
    />
  )
}

function ConfigField({
  entry,
  onReset,
  onSave,
}: {
  entry: ConfigEntry
  onReset: (key: string) => void
  onSave: (key: string, value: string) => void
}) {
  const [localValue, setLocalValue] = useState(entry.value)
  const [saving, setSaving] = useState(false)
  const [resetting, setResetting] = useState(false)

  // sync local value when entry changes from parent
  useEffect(() => {
    setLocalValue(entry.value)
  }, [entry.value])

  const dirty = localValue !== entry.value

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

  return (
    <div className="border-border border-b px-4 py-4 last:border-0">
      <div className="mb-1.5 flex flex-wrap items-center gap-2">
        <code className="text-fg text-sm font-medium">{entry.key}</code>
        <SourceBadge source={entry.source} />
        {entry.updated_by && (
          <span className="text-fg-muted text-xs">
            by {entry.updated_by}
            {entry.updated_at ? ` on ${new Date(entry.updated_at).toLocaleDateString()}` : ''}
          </span>
        )}
      </div>
      {entry.description && <p className="text-fg-secondary mb-2 text-xs">{entry.description}</p>}
      <div className="flex flex-wrap items-center gap-2">
        <ConfigControl onChange={setLocalValue} value={localValue} valueType={entry.value_type} />
        <button
          className="bg-fg text-bg inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs font-medium transition-colors hover:opacity-90 disabled:opacity-50"
          disabled={!dirty || saving}
          onClick={handleSave}
        >
          <Save className="h-3 w-3" />
          {saving ? 'Saving...' : 'Save'}
        </button>
        {entry.source === 'database' && (
          <button
            className="text-fg-secondary hover:bg-bg-secondary inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs transition-colors disabled:opacity-50"
            disabled={resetting}
            onClick={handleReset}
          >
            <RotateCcw className="h-3 w-3" />
            {resetting ? 'Resetting...' : 'Reset to default'}
          </button>
        )}
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

function SourceBadge({ source }: { source: string }) {
  const style = SOURCE_STYLES[source] ?? SOURCE_STYLES.default
  const label = SOURCE_LABELS[source] ?? source
  return <span className={`rounded px-1.5 py-0.5 text-xs font-medium ${style}`}>{label}</span>
}
