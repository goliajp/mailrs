import { useState } from 'react'

export const PRIMARY_BUTTON = 'bg-fg text-bg rounded-md px-3 py-1.5 text-sm disabled:opacity-50'
export const CANCEL_BUTTON =
  'text-fg-secondary hover:bg-bg-secondary rounded-md px-3 py-1.5 text-sm transition-colors'

export function AddAppForm({
  onCancel,
  onSave,
  saving,
  scopeList,
}: {
  onCancel: () => void
  onSave: (vars: { name: string; scopes: string[] }) => void
  saving: boolean
  scopeList: (selected: string[], onChange: (next: string[]) => void) => React.ReactNode
}) {
  const [name, setName] = useState('')
  const [scopes, setScopes] = useState<string[]>([])

  const save = () => {
    if (!name.trim()) return
    onSave({ name: name.trim(), scopes })
  }

  let saveLabel = 'Add app'
  if (saving) saveLabel = 'Adding...'

  return (
    <div className="border-border mb-4 space-y-3 rounded-lg border p-4">
      <input
        aria-label="App name"
        autoFocus
        className="border-border bg-bg-secondary w-full rounded-md border px-3 py-1.5 text-sm"
        onChange={(e) => setName(e.target.value)}
        placeholder="App name"
        value={name}
      />
      {scopeList(scopes, setScopes)}
      <div className="flex gap-2">
        <button
          className={PRIMARY_BUTTON}
          disabled={!name.trim() || saving}
          onClick={save}
          type="button"
        >
          {saveLabel}
        </button>
        <button className={CANCEL_BUTTON} onClick={onCancel} type="button">
          Cancel
        </button>
      </div>
    </div>
  )
}
