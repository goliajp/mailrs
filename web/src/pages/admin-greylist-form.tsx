import type { GreylistLocalKind, GreylistLocalListName } from '@/lib/types'

import { useState } from 'react'

import { type Key, useT } from '@/i18n'

export type GreylistForm = {
  kind: GreylistLocalKind
  list: GreylistLocalListName
  note: string
  value: string
}

const EMPTY_FORM: GreylistForm = {
  kind: 'domain',
  list: 'white',
  note: '',
  value: '',
}

const placeholderFor: Record<GreylistLocalKind, string> = {
  cidr: '203.0.113.0/24  or  2001:db8::/32',
  domain: 'gmail.com',
  email: 'user@example.com',
}

export function GreylistAddForm({
  onCancel,
  onSave,
  saving,
}: {
  onCancel: () => void
  onSave: (form: GreylistForm, done: () => void) => void
  saving: boolean
}) {
  const t = useT()
  const [form, setForm] = useState<GreylistForm>(EMPTY_FORM)
  const [formError, setFormError] = useState<Key | null>(null)

  const handleSave = () => {
    const err = validateValue(form.kind, form.value)
    setFormError(err)
    if (err) return
    onSave(form, () => setForm(EMPTY_FORM))
  }
  let saveLabel: Key = 'admin.greylist.add'
  if (saving) saveLabel = 'admin.adding'

  return (
    <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
      <p className="text-fg-secondary text-xs">
        <strong className="text-fg">{t('admin.greylist.white')}</strong>{' '}
        {t('admin.greylist.form.whiteHelp')}{' '}
        <strong className="text-fg">{t('admin.greylist.black')}</strong>{' '}
        {t('admin.greylist.form.blackHelp')}
      </p>
      <div className="flex flex-wrap gap-2">
        <select
          aria-label={t('admin.col.type')}
          className="border-border bg-bg-secondary w-28 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, kind: e.target.value as GreylistLocalKind })}
          value={form.kind}
        >
          <option value="domain">{t('admin.col.domain')}</option>
          <option value="email">{t('admin.greylist.kind.email')}</option>
          <option value="cidr">CIDR</option>
        </select>
        <select
          aria-label={t('admin.greylist.list')}
          className="border-border bg-bg-secondary w-24 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, list: e.target.value as GreylistLocalListName })}
          value={form.list}
        >
          <option value="white">{t('admin.greylist.white')}</option>
          <option value="black">{t('admin.greylist.black')}</option>
        </select>
        <input
          aria-label={t('admin.greylist.value')}
          autoFocus
          className="border-border bg-bg-secondary flex-1 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => {
            setForm({ ...form, value: e.target.value })
            setFormError(null)
          }}
          placeholder={placeholderFor[form.kind]}
          value={form.value}
        />
      </div>
      <input
        aria-label={t('admin.greylist.noteLabel')}
        className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
        onChange={(e) => setForm({ ...form, note: e.target.value })}
        placeholder={t('admin.greylist.notePlaceholder')}
        value={form.note}
      />
      {formError && <p className="text-danger text-xs">{t(formError)}</p>}
      <div className="flex gap-2">
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
          disabled={saving || !form.value.trim()}
          onClick={handleSave}
        >
          {t(saveLabel)}
        </button>
        <button
          className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
          onClick={() => {
            setForm(EMPTY_FORM)
            setFormError(null)
            onCancel()
          }}
        >
          {t('common.cancel')}
        </button>
      </div>
    </div>
  )
}

function validateValue(kind: GreylistLocalKind, value: string): Key | null {
  const v = value.trim()
  if (!v) return 'admin.greylist.err.empty'
  if (kind === 'domain') {
    if (!v.includes('.') || v.includes('@') || v.includes(' ')) {
      return 'admin.greylist.err.domain'
    }
  } else if (kind === 'email') {
    if (!/^[^@\s]+@[^@\s.]+\.[^@\s]+$/.test(v)) {
      return 'admin.greylist.err.email'
    }
  } else if (kind === 'cidr') {
    if (!v.includes('/')) return 'admin.greylist.err.cidr'
  }
  return null
}
