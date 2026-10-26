import type { MockSettings } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useId, useState } from 'react'

import { adminKeys } from '@/lib/query-keys'
import { wireSaveMockSettings } from '@/wire/endpoints/mock'

import { btnPrimary, inputClass } from '../settings/_shared'
import { errorText } from './errors'

type NumberField = {
  help: string
  key: 'max_messages' | 'max_namespaces_per_user' | 'max_total_mb' | 'retention_days'
  label: string
  max: number
  min: number
}

// the bounds the server enforces (core-sidestate families/mock/settings.rs)
const FIELDS: NumberField[] = [
  {
    help: 'Older messages are deleted. A shorter value reaches stored mail at the next sweep, within six hours; a longer one applies to new mail',
    key: 'retention_days',
    label: 'Keep messages for (days)',
    max: 30,
    min: 1,
  },
  {
    help: 'Past this, the oldest message of the namespace is dropped',
    key: 'max_messages',
    label: 'Messages per namespace',
    max: 5000,
    min: 10,
  },
  {
    help: 'Past this, new mock mail is refused until old mail expires or is deleted',
    key: 'max_total_mb',
    label: 'Total storage (MB)',
    max: 20480,
    min: 64,
  },
  {
    help: 'How many namespaces one person may create',
    key: 'max_namespaces_per_user',
    label: 'Namespaces per person',
    max: 100,
    min: 1,
  },
]

/** The rules mock mail is kept by. Saved settings apply at once, with no restart. */
export function MockSettingsForm({ saved }: { saved: MockSettings }) {
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState<MockSettings>(saved)
  const enabledId = useId()
  const save = useMutation({
    mutationFn: () => wireSaveMockSettings(draft),
    onError: (e) => toast.error(errorText(e, 'Could not save the settings')),
    onSuccess: () => {
      toast.success('Settings saved')
      void queryClient.invalidateQueries({ queryKey: adminKeys.mock() })
    },
  })
  const invalid = firstInvalid(draft)
  const changed = JSON.stringify(draft) !== JSON.stringify(saved)

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    if (invalid !== null || !changed || save.isPending) return
    save.mutate()
  }

  return (
    <form aria-label="Mock mail settings" className="max-w-xl space-y-5" onSubmit={submit}>
      <div className="flex items-start gap-3">
        <input
          aria-describedby={`${enabledId}-help`}
          checked={draft.enabled}
          className="mt-1"
          id={enabledId}
          onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
          type="checkbox"
        />
        <div>
          <label className="text-fg block text-sm" htmlFor={enabledId}>
            Accept mock mail
          </label>
          <p className="text-fg-muted text-xs" id={`${enabledId}-help`}>
            When off, new mail to the mock domain is refused and no namespace can be created. Stored
            mail stays readable
          </p>
        </div>
      </div>
      <fieldset className="space-y-1">
        <legend className="text-fg text-sm">Who can create namespaces</legend>
        <CreateChoice draft={draft} onChange={setDraft} value="all-users">
          Everyone who can sign in
        </CreateChoice>
        <CreateChoice draft={draft} onChange={setDraft} value="admins-only">
          Admins only
        </CreateChoice>
      </fieldset>
      <div className="grid gap-4 sm:grid-cols-2">
        {FIELDS.map((f) => (
          <NumberInput
            field={f}
            key={f.key}
            onChange={(v) => setDraft({ ...draft, [f.key]: v })}
            value={draft[f.key]}
          />
        ))}
      </div>
      {invalid !== null && (
        <p className="text-danger text-sm" role="alert">
          {invalid}
        </p>
      )}
      <button
        className={btnPrimary}
        disabled={invalid !== null || !changed || save.isPending}
        type="submit"
      >
        Save settings
      </button>
    </form>
  )
}

function CreateChoice({
  children,
  draft,
  onChange,
  value,
}: {
  children: React.ReactNode
  draft: MockSettings
  onChange: (s: MockSettings) => void
  value: MockSettings['create']
}) {
  return (
    <label className="text-fg-secondary flex items-center gap-2 text-sm">
      <input
        checked={draft.create === value}
        name="create"
        onChange={() => onChange({ ...draft, create: value })}
        type="radio"
      />
      {children}
    </label>
  )
}

function firstInvalid(s: MockSettings): null | string {
  for (const f of FIELDS) {
    const v = s[f.key]
    if (!Number.isInteger(v) || v < f.min || v > f.max) {
      return `${f.label} must be a whole number from ${f.min} to ${f.max}`
    }
  }
  return null
}

function NumberInput({
  field,
  onChange,
  value,
}: {
  field: NumberField
  onChange: (v: number) => void
  value: number
}) {
  const id = useId()
  return (
    <div>
      <label className="text-fg block text-sm" htmlFor={id}>
        {field.label}
      </label>
      <input
        aria-describedby={`${id}-help`}
        className={`${inputClass} mt-1 tabular-nums`}
        id={id}
        max={field.max}
        min={field.min}
        onChange={(e) => onChange(Number(e.target.value))}
        type="number"
        value={value}
      />
      <p className="text-fg-muted mt-1 text-xs" id={`${id}-help`}>
        {field.help}. {field.min} to {field.max}
      </p>
    </div>
  )
}
