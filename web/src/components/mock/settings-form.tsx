import type { MockSettings } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useId, useState } from 'react'

import { type Key, useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { wireSaveMockSettings } from '@/wire/endpoints/mock'

import { btnPrimary, inputClass } from '../settings/_shared'
import { errorText } from './errors'

type NumberField = {
  help: Key
  key: 'max_messages' | 'max_namespaces_per_user' | 'max_total_mb' | 'retention_days'
  label: Key
  max: number
  min: number
}

// the bounds the server enforces (core-sidestate families/mock/settings.rs)
const FIELDS: NumberField[] = [
  {
    help: 'settings.mock.retention.help',
    key: 'retention_days',
    label: 'settings.mock.retention',
    max: 30,
    min: 1,
  },
  {
    help: 'settings.mock.messages.help',
    key: 'max_messages',
    label: 'settings.mock.messages',
    max: 5000,
    min: 10,
  },
  {
    help: 'settings.mock.storage.help',
    key: 'max_total_mb',
    label: 'settings.mock.storage',
    max: 20480,
    min: 64,
  },
  {
    help: 'settings.mock.namespaces.help',
    key: 'max_namespaces_per_user',
    label: 'settings.mock.namespaces',
    max: 100,
    min: 1,
  },
]

/** The rules mock mail is kept by. Saved settings apply at once, with no restart. */
export function MockSettingsForm({ saved }: { saved: MockSettings }) {
  const t = useT()
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState<MockSettings>(saved)
  const enabledId = useId()
  const save = useMutation({
    mutationFn: () => wireSaveMockSettings(draft),
    onError: (e) => toast.error(errorText(e, t('settings.mock.saveFailed'))),
    onSuccess: () => {
      toast.success(t('settings.mock.saved'))
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
    <form aria-label={t('settings.mock.form')} className="max-w-xl space-y-5" onSubmit={submit}>
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
            {t('settings.mock.accept')}
          </label>
          <p className="text-fg-muted text-xs" id={`${enabledId}-help`}>
            {t('settings.mock.acceptHelp')}
          </p>
        </div>
      </div>
      <fieldset className="space-y-1">
        <legend className="text-fg text-sm">{t('settings.mock.who')}</legend>
        <CreateChoice draft={draft} onChange={setDraft} value="all-users">
          {t('settings.mock.everyone')}
        </CreateChoice>
        <CreateChoice draft={draft} onChange={setDraft} value="admins-only">
          {t('settings.mock.adminsOnly')}
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
          {t('settings.mock.invalid', {
            label: t(invalid.label),
            max: invalid.max,
            min: invalid.min,
          })}
        </p>
      )}
      <button
        className={btnPrimary}
        disabled={invalid !== null || !changed || save.isPending}
        type="submit"
      >
        {t('settings.mock.save')}
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

function firstInvalid(s: MockSettings): null | NumberField {
  for (const f of FIELDS) {
    const v = s[f.key]
    if (!Number.isInteger(v) || v < f.min || v > f.max) return f
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
  const t = useT()
  const id = useId()
  return (
    <div>
      <label className="text-fg block text-sm" htmlFor={id}>
        {t(field.label)}
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
        {t('settings.mock.fieldHelp', { help: t(field.help), max: field.max, min: field.min })}
      </p>
    </div>
  )
}
