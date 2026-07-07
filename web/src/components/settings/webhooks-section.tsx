import type { CreatedWebhook, Webhook } from './_shared'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { deleteJson, fetchList, postJson } from '@/lib/api'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'

import {
  btnPrimary,
  btnSecondary,
  cardClass,
  ConfirmDialog,
  inputClass,
  SectionHeader,
} from './_shared'

export function WebhooksSection() {
  const { data: webhooks = [] } = useQuery({
    queryKey: settingsKeys.webhooks(),
    queryFn: () => fetchList<Webhook>('/agent/webhooks'),
  })
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({
    event_type: 'new_message',
    filter_sender: '',
    filter_thread_id: '',
    url: '',
  })
  const [createdSecret, setCreatedSecret] = useState<null | string>(null)
  const [deleteTarget, setDeleteTarget] = useState<null | string>(null)
  const [creating, setCreating] = useState(false)

  const invalidate = () => queryClient.invalidateQueries({ queryKey: settingsKeys.webhooks() })

  const handleCreate = async () => {
    if (!form.url.trim() || creating) return
    setCreating(true)
    try {
      const data = await postJson<CreatedWebhook>('/agent/webhooks', {
        event_type: form.event_type,
        url: form.url.trim(),
        ...(form.filter_sender.trim() ? { filter_sender: form.filter_sender.trim() } : {}),
        ...(form.filter_thread_id.trim() ? { filter_thread_id: form.filter_thread_id.trim() } : {}),
      })
      toast.success('Webhook created')
      setCreatedSecret(data.signing_secret)
      setForm({
        event_type: 'new_message',
        filter_sender: '',
        filter_thread_id: '',
        url: '',
      })
      setAdding(false)
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create webhook')
    } finally {
      setCreating(false)
    }
  }

  const handleDelete = async (id: string) => {
    try {
      await deleteJson(`/agent/webhooks/${id}`)
      toast.success('Webhook deleted')
      setDeleteTarget(null)
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete webhook')
      setDeleteTarget(null)
    }
  }

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text).then(() => toast.success('Copied to clipboard'))
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title="Webhooks" />
        <button className={btnPrimary} onClick={() => setAdding(true)}>
          Add Webhook
        </button>
      </div>

      {createdSecret && (
        <div className="border-warning bg-warning/10 rounded-lg border p-4" role="status">
          <p className="mb-2 text-sm font-semibold">Signing Secret</p>
          <p className="text-fg-secondary mb-2 text-xs">
            Copy this secret now. It will not be shown again.
          </p>
          <div className="flex items-center gap-2">
            <code className="bg-bg-secondary flex-1 rounded px-3 py-1.5 font-mono text-sm">
              {createdSecret}
            </code>
            <button className={btnPrimary} onClick={() => copyToClipboard(createdSecret)}>
              Copy
            </button>
          </div>
          <button
            className="text-fg-secondary mt-2 text-xs transition-colors hover:opacity-70"
            onClick={() => setCreatedSecret(null)}
          >
            Dismiss
          </button>
        </div>
      )}

      {adding && (
        <div className={cardClass + ' space-y-3'}>
          <input
            aria-label="Webhook URL"
            autoFocus
            className={inputClass}
            onChange={(e) => setForm({ ...form, url: e.target.value })}
            placeholder="https://example.com/webhook"
            type="url"
            value={form.url}
          />
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">Event type</label>
            <select
              aria-label="Event type"
              className={inputClass}
              onChange={(e) => setForm({ ...form, event_type: e.target.value })}
              value={form.event_type}
            >
              <option value="new_message">new_message</option>
              <option value="message_read">message_read</option>
              <option value="message_deleted">message_deleted</option>
            </select>
          </div>
          <input
            aria-label="Filter by sender"
            className={inputClass}
            onChange={(e) => setForm({ ...form, filter_sender: e.target.value })}
            placeholder="Filter by sender (optional)"
            value={form.filter_sender}
          />
          <input
            aria-label="Filter by thread ID"
            className={inputClass}
            onChange={(e) => setForm({ ...form, filter_thread_id: e.target.value })}
            placeholder="Filter by thread ID (optional)"
            value={form.filter_thread_id}
          />
          <div className="flex gap-2">
            <button className={btnPrimary} disabled={creating} onClick={handleCreate}>
              {creating ? 'Creating…' : 'Create'}
            </button>
            <button className={btnSecondary} onClick={() => setAdding(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {webhooks.length === 0 && !adding && (
        <p className="text-fg-muted text-sm">No webhooks configured</p>
      )}

      {webhooks.map((wh) => (
        <div className={cardClass} key={wh.id}>
          <div className="flex items-start justify-between">
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium" title={wh.url}>
                {wh.url}
              </p>
              <div className="mt-1 flex flex-wrap gap-2">
                <span className="bg-accent/10 text-accent rounded-full px-2 py-0.5 text-xs">
                  {wh.event_type}
                </span>
                {wh.filter_sender && (
                  <span className="bg-surface text-fg-muted rounded-full px-2 py-0.5 text-xs">
                    sender: {wh.filter_sender}
                  </span>
                )}
                {wh.filter_thread_id && (
                  <span className="bg-surface text-fg-muted rounded-full px-2 py-0.5 text-xs">
                    thread: {wh.filter_thread_id}
                  </span>
                )}
                <span
                  className={`rounded-full px-2 py-0.5 text-xs font-medium ${
                    wh.active ? 'bg-success/10 text-success' : 'bg-surface text-fg-muted'
                  }`}
                >
                  {wh.active ? 'Active' : 'Inactive'}
                </span>
              </div>
            </div>
            <button className="text-danger ml-3 text-xs" onClick={() => setDeleteTarget(wh.id)}>
              Delete
            </button>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message="Delete this webhook? This cannot be undone."
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
        />
      )}
    </div>
  )
}
