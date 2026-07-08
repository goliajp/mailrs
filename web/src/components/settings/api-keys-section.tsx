import type { CreatedAgentKey } from './_shared'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  wireCreateAgentKey,
  wireDeleteAgentKey,
  wireListAgentKeys,
} from '@/wire/endpoints/settings'

import {
  btnPrimary,
  btnSecondary,
  cardClass,
  ConfirmDialog,
  inputClass,
  SectionHeader,
} from './_shared'

export function ApiKeysSection() {
  const { data: keys = [] } = useQuery({
    queryKey: settingsKeys.agentKeys(),
    queryFn: () => wireListAgentKeys().then((items) => [...items]),
  })
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({ expires_in_days: '', name: '' })
  const [createdKey, setCreatedKey] = useState<CreatedAgentKey | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)

  const invalidate = () => queryClient.invalidateQueries({ queryKey: settingsKeys.agentKeys() })

  const handleCreate = async () => {
    if (!form.name.trim()) return
    try {
      const expires_in_days = form.expires_in_days ? parseInt(form.expires_in_days, 10) : undefined
      const data = await wireCreateAgentKey({ expires_in_days, name: form.name.trim() })
      toast.success('API key created')
      setCreatedKey(data)
      setForm({ expires_in_days: '', name: '' })
      setAdding(false)
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create key')
    }
  }

  const handleRevoke = async (id: string) => {
    try {
      await wireDeleteAgentKey(id)
      toast.success('API key revoked')
      setDeleteTarget(null)
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to revoke key')
      setDeleteTarget(null)
    }
  }

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text).then(() => toast.success('Copied to clipboard'))
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title="API Keys" />
        <button className={btnPrimary} onClick={() => setAdding(true)}>
          Create Key
        </button>
      </div>

      {createdKey && (
        <div className="border-warning bg-warning/10 rounded-lg border p-4" role="status">
          <p className="mb-2 text-sm font-semibold">API Key Created</p>
          <p className="text-fg-secondary mb-2 text-xs">
            Copy this key now. It will not be shown again.
          </p>
          <div className="flex items-center gap-2">
            <code className="bg-bg-secondary flex-1 rounded px-3 py-1.5 font-mono text-sm">
              {createdKey.key}
            </code>
            <button className={btnPrimary} onClick={() => copyToClipboard(createdKey.key)}>
              Copy
            </button>
          </div>
          <button
            className="text-fg-secondary mt-2 text-xs transition-colors hover:opacity-70"
            onClick={() => setCreatedKey(null)}
          >
            Dismiss
          </button>
        </div>
      )}

      {adding && (
        <div className={cardClass + ' space-y-3'}>
          <input
            aria-label="Key name"
            autoFocus
            className={inputClass}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="Key name"
            value={form.name}
          />
          <input
            aria-label="Expires in days"
            className={inputClass}
            min="1"
            onChange={(e) => setForm({ ...form, expires_in_days: e.target.value })}
            placeholder="Expires in days (optional)"
            type="number"
            value={form.expires_in_days}
          />
          <div className="flex gap-2">
            <button className={btnPrimary} onClick={handleCreate}>
              Create
            </button>
            <button className={btnSecondary} onClick={() => setAdding(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {keys.length === 0 && !adding && <p className="text-fg-muted text-sm">No API keys</p>}

      {keys.map((k) => (
        <div className={cardClass} key={k.id}>
          <div className="flex items-center justify-between">
            <div>
              <span className="text-sm font-medium">{k.name}</span>
              <span className="text-fg-muted ml-2 font-mono text-xs">{k.prefix}...</span>
              {k.expires_at && (
                <span className="text-fg-muted ml-2 text-xs">
                  expires {new Date(k.expires_at).toLocaleDateString()}
                </span>
              )}
            </div>
            <button className="text-danger text-xs" onClick={() => setDeleteTarget(k.id)}>
              Revoke
            </button>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message="Revoke this API key? This cannot be undone."
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleRevoke(deleteTarget)}
        />
      )}
    </div>
  )
}
