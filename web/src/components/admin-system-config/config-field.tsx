import type { ConfigEntry } from './entry'

import { useEffect, useState } from 'react'

import { resetVerb } from './entry'
import { ReadOnlyField } from './read-only-field'

const SOURCE_STYLES: Record<string, string> = {
  database: 'bg-accent/10 text-accent',
  default: 'bg-bg-secondary text-fg-muted',
  env: 'bg-warning/10 text-warning',
  unknown: 'bg-bg-secondary text-fg-muted',
}

// "database" is the wire name; the value is kept in the network kevy hash
const SOURCE_LABELS: Record<string, string> = {
  database: 'Saved here',
  default: 'Default',
  env: 'Environment',
  unknown: 'Not visible here',
}

/** One setting: an input when a running process reads it, otherwise where it is set. */
export function ConfigField({
  entry,
  onReset,
  onSave,
  saving,
}: {
  entry: ConfigEntry
  onReset: (entry: ConfigEntry) => void
  onSave: (key: string, value: string) => void
  saving: boolean
}) {
  const [localValue, setLocalValue] = useState(entry.value ?? '')

  // a draft follows the stored value whenever that changes
  useEffect(() => {
    setLocalValue(entry.value ?? '')
  }, [entry.value])

  const dirty = localValue !== (entry.value ?? '')
  const saved = savedBy(entry)

  const resetButton = entry.source === 'database' && (
    <button
      className="text-danger text-xs hover:opacity-70"
      onClick={() => onReset(entry)}
      type="button"
    >
      {resetVerb(entry)}
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
      <div className="flex flex-wrap items-center gap-3">
        <input
          aria-label={entry.key}
          className="border-border bg-bg-secondary w-full max-w-md rounded-md border px-3 py-1.5 text-sm tabular-nums"
          onChange={(e) => setLocalValue(e.target.value)}
          type="text"
          value={localValue}
        />
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-xs font-medium transition-colors hover:opacity-90 disabled:opacity-50"
          disabled={!dirty || saving}
          onClick={() => onSave(entry.key, localValue)}
          type="button"
        >
          {saveLabel(saving)}
        </button>
        {resetButton}
      </div>
    </div>
  )
}

function savedBy(entry: ConfigEntry): null | string {
  if (entry.updated_at === undefined) return null
  const on = new Date(entry.updated_at * 1000).toLocaleString()
  if (entry.updated_by) return `Saved by ${entry.updated_by} on ${on}`
  return `Saved on ${on}`
}

function saveLabel(saving: boolean): string {
  if (saving) return 'Saving...'
  return 'Save'
}

function SourceBadge({ source }: { source: string }) {
  const style = SOURCE_STYLES[source] ?? SOURCE_STYLES.default
  const label = SOURCE_LABELS[source] ?? source
  return <span className={`rounded px-1.5 py-0.5 text-xs font-medium ${style}`}>{label}</span>
}
