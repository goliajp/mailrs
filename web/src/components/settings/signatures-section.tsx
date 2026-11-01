import type { Signature } from './_shared'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { type Key, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  wireCreateSignature,
  wireDeleteSignature,
  wireListSignatures,
} from '@/wire/endpoints/settings'

import {
  btnPrimary,
  btnSecondary,
  cardClass,
  ConfirmDialog,
  inputClass,
  SectionHeader,
} from './_shared'

export function SignaturesSection() {
  const t = useT()
  const { data: signatures = [] } = useQuery({
    queryKey: settingsKeys.signatures(),
    queryFn: () => wireListSignatures().then((items) => [...items]),
  })
  const [editing, setEditing] = useState<null | Partial<Signature>>(null)
  const [saving, setSaving] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)

  const invalidate = () => queryClient.invalidateQueries({ queryKey: settingsKeys.signatures() })

  const handleSave = async () => {
    if (!editing?.name?.trim()) return
    setSaving(true)
    try {
      await wireCreateSignature({
        html_content: editing.html_content ?? '',
        id: editing.id,
        is_default: editing.is_default ?? false,
        name: editing.name.trim(),
        text_content: editing.text_content ?? '',
      })
      toast.success(t(savedLabel(Boolean(editing.id))))
      setEditing(null)
      void invalidate()
    } catch (e) {
      toast.error(errorText(e, t('settings.signatures.saveFailed')))
    } finally {
      setSaving(false)
    }
  }

  const handleDelete = async (id: number) => {
    try {
      await wireDeleteSignature(id)
      toast.success(t('settings.signatures.deleted'))
      setDeleteTarget(null)
      void invalidate()
    } catch (e) {
      toast.error(errorText(e, t('settings.signatures.deleteFailed')))
      setDeleteTarget(null)
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title={t('settings.cat.signatures')} />
        <button
          className={btnPrimary}
          onClick={() =>
            setEditing({
              html_content: '',
              is_default: false,
              name: '',
              text_content: '',
            })
          }
        >
          {t('settings.signatures.add')}
        </button>
      </div>

      {editing && (
        <div className={cardClass + ' space-y-3'}>
          <input
            aria-label={t('settings.signatures.name')}
            autoFocus
            className={inputClass}
            onChange={(e) => setEditing({ ...editing, name: e.target.value })}
            placeholder={t('settings.signatures.name')}
            value={editing.name ?? ''}
          />
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">
              {t('settings.signatures.text')}
            </label>
            <textarea
              aria-label={t('settings.signatures.text')}
              className={inputClass}
              onChange={(e) => setEditing({ ...editing, text_content: e.target.value })}
              rows={3}
              value={editing.text_content ?? ''}
            />
          </div>
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">
              {t('settings.signatures.html')}
            </label>
            <textarea
              aria-label={t('settings.signatures.html')}
              className={inputClass}
              onChange={(e) => setEditing({ ...editing, html_content: e.target.value })}
              rows={3}
              value={editing.html_content ?? ''}
            />
          </div>
          <label className="flex items-center gap-2 text-sm">
            <input
              checked={editing.is_default ?? false}
              onChange={(e) => setEditing({ ...editing, is_default: e.target.checked })}
              type="checkbox"
            />
            {t('settings.signatures.defaultSignature')}
          </label>
          <div className="flex gap-2">
            <button className={btnPrimary} disabled={saving} onClick={handleSave}>
              {t(saveLabel(saving))}
            </button>
            <button className={btnSecondary} onClick={() => setEditing(null)}>
              {t('common.cancel')}
            </button>
          </div>
        </div>
      )}

      {signatures.length === 0 && !editing && (
        <p className="text-fg-muted text-sm">{t('settings.signatures.empty')}</p>
      )}

      {signatures.map((sig) => (
        <div className={cardClass} key={sig.id}>
          <div className="flex items-start justify-between">
            <div>
              <span className="text-sm font-medium">{sig.name}</span>
              {sig.is_default && (
                <span className="bg-success/10 text-success ml-2 rounded-full px-2 py-0.5 text-xs">
                  {t('settings.signatures.default')}
                </span>
              )}
              {sig.text_content && (
                <p className="text-fg-muted mt-1 text-xs whitespace-pre-wrap">
                  {sig.text_content.slice(0, 120)}
                  {sig.text_content.length > 120 ? '...' : ''}
                </p>
              )}
            </div>
            <div className="flex gap-2">
              <button className="text-accent text-xs" onClick={() => setEditing(sig)}>
                {t('settings.signatures.edit')}
              </button>
              <button className="text-danger text-xs" onClick={() => setDeleteTarget(sig.id)}>
                {t('settings.rules.delete')}
              </button>
            </div>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message={t('settings.signatures.deleteConfirm')}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
        />
      )}
    </div>
  )
}

function savedLabel(existing: boolean): Key {
  if (existing) return 'settings.signatures.updated'
  return 'settings.signatures.created'
}

function saveLabel(saving: boolean): Key {
  if (saving) return 'settings.saving'
  return 'settings.save'
}
