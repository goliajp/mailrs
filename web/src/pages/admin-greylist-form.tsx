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
  let saveLabel = 'Save'
  if (saving) saveLabel = 'Saving…'

  return (
    <div className="border-border mb-4 space-y-2 rounded-lg border p-4">
      <div className="text-fg-secondary text-xs">
        <strong className="text-fg">white</strong> = skip greylist (Gmail/Outlook etc. already
        covered by remote whitelist). <strong className="text-fg">black</strong> = reject with SMTP
        550 5.7.1. A value can be on either list, never both.
      </div>
      <div className="flex flex-wrap gap-2">
        <select
          aria-label="Kind"
          className="border-border bg-bg-secondary w-28 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, kind: e.target.value as GreylistLocalKind })}
          value={form.kind}
        >
          <option value="domain">domain</option>
          <option value="email">email</option>
          <option value="cidr">cidr</option>
        </select>
        <select
          aria-label="List"
          className="border-border bg-bg-secondary w-24 rounded-md border px-3 py-1.5 text-sm"
          onChange={(e) => setForm({ ...form, list: e.target.value as GreylistLocalListName })}
          value={form.list}
        >
          <option value="white">white</option>
          <option value="black">black</option>
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
        placeholder="optional note — why this entry exists"
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
  if (!v) return 'value is required'
  if (kind === 'domain') {
    if (!v.includes('.') || v.includes('@') || v.includes(' ')) {
      return 'invalid domain (need at least one dot, no @, no spaces)'
    }
  } else if (kind === 'email') {
    if (!/^[^@\s]+@[^@\s.]+\.[^@\s]+$/.test(v)) {
      return 'invalid email (need user@host.tld)'
    }
  } else if (kind === 'cidr') {
    if (!v.includes('/')) return 'cidr needs a /N suffix'
  }
  return null
}
