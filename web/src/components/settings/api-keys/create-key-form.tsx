import type { ScopePreset } from './scopes'

import { useState } from 'react'

import { btnPrimary, btnSecondary, inputClass } from '../_shared'
import { SCOPE_PRESETS } from './scopes'

/**
 * Create form. Fields mirror what `create_agent_key` actually reads —
 * `{name, scopes}`. There is no expiry field because the endpoint has no
 * expiry concept; the previous "expires in days" input was discarded
 * server-side and shown as if it had taken effect.
 */
export function CreateKeyForm({
  busy,
  onCancel,
  onCreate,
}: {
  busy: boolean
  onCancel: () => void
  onCreate: (values: { name: string; preset: ScopePreset }) => void
}) {
  const [name, setName] = useState('')
  const [preset, setPreset] = useState<ScopePreset>('full')

  const canSubmit = name.trim().length > 0 && !busy

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    onCreate({ name: name.trim(), preset })
  }

  return (
    <form
      className="border-border bg-surface shadow-elevation-1 rounded-lg border p-4"
      onSubmit={handleSubmit}
    >
      <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
        <label className="block">
          <span className="text-fg-secondary mb-1.5 block text-xs font-medium">Name</span>
          <input
            autoFocus
            className={inputClass}
            onChange={(e) => setName(e.target.value)}
            placeholder="ci-deploy-bot"
            value={name}
          />
          <span className="text-fg-muted text-mini mt-1.5 block">
            Shown in this table and in audit entries. Name it after the thing that holds it.
          </span>
        </label>

        <fieldset>
          <legend className="text-fg-secondary mb-1.5 block text-xs font-medium">Access</legend>
          <div className="space-y-1">
            {SCOPE_PRESETS.map((option) => (
              <ScopeOption
                checked={option.value === preset}
                description={option.description}
                key={option.value}
                label={option.label}
                onSelect={() => setPreset(option.value)}
              />
            ))}
          </div>
        </fieldset>
      </div>

      <div className="mt-4 flex gap-2">
        <button className={btnPrimary} disabled={!canSubmit} type="submit">
          Create key
        </button>
        <button className={btnSecondary} onClick={onCancel} type="button">
          Cancel
        </button>
      </div>
    </form>
  )
}

function optionClass(checked: boolean): string {
  const base = 'flex cursor-pointer items-start gap-2 rounded-md border px-3 py-2 transition-colors'
  if (checked) return `${base} border-accent bg-accent/5`
  return `${base} border-border hover:bg-bg-secondary`
}

function ScopeOption({
  checked,
  description,
  label,
  onSelect,
}: {
  checked: boolean
  description: string
  label: string
  onSelect: () => void
}) {
  return (
    <label className={optionClass(checked)}>
      <input
        checked={checked}
        className="accent-accent mt-0.5"
        name="agent-key-scope"
        onChange={onSelect}
        type="radio"
      />
      <span className="min-w-0">
        <span className="block text-sm">{label}</span>
        <span className="text-fg-muted text-mini block">{description}</span>
      </span>
    </label>
  )
}
