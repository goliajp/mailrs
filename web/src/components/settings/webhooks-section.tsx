import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { type Key, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import { wireCreateWebhook, wireDeleteWebhook, wireListWebhooks } from '@/wire/endpoints/settings'

import {
  btnPrimary,
  btnSecondary,
  cardClass,
  ConfirmDialog,
  inputClass,
  SectionHeader,
} from './_shared'

/** The events a subscription can name (crates/core-api/src/webhook_events.rs, `ALL`). */
const EVENTS = [
  'mail.received',
  'mail.held',
  'mail.sent',
  'mail.delivered',
  'mail.deferred',
  'mail.bounced',
  'thread.read',
  'thread.archived',
  'thread.trashed',
  'thread.starred',
  'thread.labelled',
  'calendar.invite_answered',
] as const

export function WebhooksSection() {
  const t = useT()
  const { data: webhooks = [] } = useQuery({
    queryKey: settingsKeys.webhooks(),
    queryFn: () => wireListWebhooks().then((items) => [...items]),
  })
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({
    event_type: 'mail.received',
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
      const data = await wireCreateWebhook({
        event_type: form.event_type,
        url: form.url.trim(),
        ...(form.filter_sender.trim() ? { filter_sender: form.filter_sender.trim() } : {}),
        ...(form.filter_thread_id.trim() ? { filter_thread_id: form.filter_thread_id.trim() } : {}),
      })
      toast.success(t('settings.webhooks.created'))
      setCreatedSecret(data.signing_secret)
      setForm({
        event_type: 'mail.received',
        filter_sender: '',
        filter_thread_id: '',
        url: '',
      })
      setAdding(false)
      void invalidate()
    } catch (e) {
      toast.error(errorText(e, t('settings.webhooks.createFailed')))
    } finally {
      setCreating(false)
    }
  }

  const handleDelete = async (id: string) => {
    try {
      await wireDeleteWebhook(id)
      toast.success(t('settings.webhooks.deleted'))
      setDeleteTarget(null)
      void invalidate()
    } catch (e) {
      toast.error(errorText(e, t('settings.webhooks.deleteFailed')))
      setDeleteTarget(null)
    }
  }

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text).then(() => toast.success(t('settings.copiedToClipboard')))
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title={t('settings.cat.webhooks')} />
        <button className={btnPrimary} onClick={() => setAdding(true)}>
          {t('settings.webhooks.add')}
        </button>
      </div>

      {createdSecret && (
        <div className="border-warning bg-warning/10 rounded-lg border p-4" role="status">
          <p className="mb-2 text-sm font-semibold">{t('settings.webhooks.secret')}</p>
          <p className="text-fg-secondary mb-2 text-xs">{t('settings.webhooks.secretOnce')}</p>
          <div className="flex items-center gap-2">
            <code className="bg-bg-secondary flex-1 rounded px-3 py-1.5 font-mono text-sm">
              {createdSecret}
            </code>
            <button className={btnPrimary} onClick={() => copyToClipboard(createdSecret)}>
              {t('common.copy')}
            </button>
          </div>
          <button
            className="text-fg-secondary mt-2 text-xs transition-colors hover:opacity-70"
            onClick={() => setCreatedSecret(null)}
          >
            {t('compose.dismiss')}
          </button>
        </div>
      )}

      {adding && (
        <div className={cardClass + ' space-y-3'}>
          <input
            aria-label={t('settings.webhooks.url')}
            autoFocus
            className={inputClass}
            onChange={(e) => setForm({ ...form, url: e.target.value })}
            placeholder="https://example.com/webhook"
            type="url"
            value={form.url}
          />
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">
              {t('settings.webhooks.eventType')}
            </label>
            <select
              aria-label={t('settings.webhooks.eventType')}
              className={inputClass}
              onChange={(e) => setForm({ ...form, event_type: e.target.value })}
              value={form.event_type}
            >
              {EVENTS.map((ev) => (
                <option key={ev} value={ev}>
                  {ev}
                </option>
              ))}
            </select>
          </div>
          <input
            aria-label={t('settings.webhooks.filterSender')}
            className={inputClass}
            onChange={(e) => setForm({ ...form, filter_sender: e.target.value })}
            placeholder={t('settings.webhooks.filterSenderOptional')}
            value={form.filter_sender}
          />
          <input
            aria-label={t('settings.webhooks.filterThread')}
            className={inputClass}
            onChange={(e) => setForm({ ...form, filter_thread_id: e.target.value })}
            placeholder={t('settings.webhooks.filterThreadOptional')}
            value={form.filter_thread_id}
          />
          <div className="flex gap-2">
            <button className={btnPrimary} disabled={creating} onClick={handleCreate}>
              {t(createLabel(creating))}
            </button>
            <button className={btnSecondary} onClick={() => setAdding(false)}>
              {t('common.cancel')}
            </button>
          </div>
        </div>
      )}

      {webhooks.length === 0 && !adding && (
        <p className="text-fg-muted text-sm">{t('settings.webhooks.empty')}</p>
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
                    {t('settings.webhooks.senderChip', { value: wh.filter_sender })}
                  </span>
                )}
                {wh.filter_thread_id && (
                  <span className="bg-surface text-fg-muted rounded-full px-2 py-0.5 text-xs">
                    {t('settings.webhooks.threadChip', { value: wh.filter_thread_id })}
                  </span>
                )}
                <span
                  className={`rounded-full px-2 py-0.5 text-xs font-medium ${
                    wh.active ? 'bg-success/10 text-success' : 'bg-surface text-fg-muted'
                  }`}
                >
                  {t(activeLabel(wh.active))}
                </span>
              </div>
            </div>
            <button className="text-danger ml-3 text-xs" onClick={() => setDeleteTarget(wh.id)}>
              {t('settings.rules.delete')}
            </button>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message={t('settings.webhooks.deleteConfirm')}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
        />
      )}
    </div>
  )
}

function activeLabel(active: boolean): Key {
  if (active) return 'settings.active'
  return 'settings.inactive'
}

function createLabel(creating: boolean): Key {
  if (creating) return 'settings.creating'
  return 'settings.create'
}
