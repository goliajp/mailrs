import type { GreylistLocalKind, GreylistLocalListName } from '@/lib/types'

import { useState } from 'react'

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
  const [form, setForm] = useState<GreylistForm>(EMPTY_FORM)
  const [formError, setFormError] = useState<null | string>(null)

  const handleSave = () => {
    const err = validateValue(form.kind, form.value)
    setFormError(err)
    if (err) return
    onSave(form, () => setForm(EMPTY_FORM))
  }
  let saveLabel = 'Add entry'
  if (saving) saveLabel = 'Adding...'

  return (
    <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
      <p className="text-fg-secondary text-xs">
        <strong className="text-fg">White</strong> skips greylisting.{' '}
        <strong className="text-fg">Black</strong> refuses the mail with SMTP 550 5.7.1. A value on
        both lists is treated as black. A domain also covers its subdomains
      </p>
      <div className="flex flex-wrap gap-2">
        <select
          aria-label="Kind"
          className="border-border bg-bg-secondary w-28 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, kind: e.target.value as GreylistLocalKind })}
          value={form.kind}
        >
          <option value="domain">Domain</option>
          <option value="email">Email</option>
          <option value="cidr">CIDR</option>
        </select>
        <select
          aria-label="List"
          className="border-border bg-bg-secondary w-24 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, list: e.target.value as GreylistLocalListName })}
          value={form.list}
        >
          <option value="white">White</option>
          <option value="black">Black</option>
        </select>
        <input
          aria-label="Value"
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
        aria-label="Note (optional)"
        className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
        onChange={(e) => setForm({ ...form, note: e.target.value })}
        placeholder="Why this entry exists (optional)"
        value={form.note}
      />
      {formError && <p className="text-danger text-xs">{formError}</p>}
      <div className="flex gap-2">
        <button
          className="bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
          disabled={saving || !form.value.trim()}
          onClick={handleSave}
        >
          {saveLabel}
        </button>
        <button
          className="text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors"
          onClick={() => {
            setForm(EMPTY_FORM)
            setFormError(null)
            onCancel()
          }}
        >
          Cancel
        </button>
      </div>
    </div>
  )
}

function validateValue(kind: GreylistLocalKind, value: string): null | string {
  const v = value.trim()
  if (!v) return 'Enter a value'
  if (kind === 'domain') {
    if (!v.includes('.') || v.includes('@') || v.includes(' ')) {
      return 'A domain needs at least one dot, and no @ or spaces'
    }
  } else if (kind === 'email') {
    if (!/^[^@\s]+@[^@\s.]+\.[^@\s]+$/.test(v)) {
      return 'An email address looks like user@example.com'
    }
  } else if (kind === 'cidr') {
    if (!v.includes('/')) return 'A CIDR needs a /N prefix length, e.g. 203.0.113.0/24'
  }
  return null
}
