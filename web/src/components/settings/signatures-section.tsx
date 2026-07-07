import type { Signature } from './_shared'

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

export function SignaturesSection() {
  const { data: signatures = [] } = useQuery({
    queryKey: settingsKeys.signatures(),
    queryFn: () => fetchList<Signature>('/mail/signatures'),
  })
  const [editing, setEditing] = useState<null | Partial<Signature>>(null)
  const [saving, setSaving] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<null | number>(null)

  const invalidate = () => queryClient.invalidateQueries({ queryKey: settingsKeys.signatures() })

  const handleSave = async () => {
    if (!editing?.name?.trim()) return
    setSaving(true)
    try {
      await postJson('/mail/signatures', {
        html_content: editing.html_content ?? '',
        id: editing.id,
        is_default: editing.is_default ?? false,
        name: editing.name.trim(),
        text_content: editing.text_content ?? '',
      })
      toast.success(editing.id ? 'Signature updated' : 'Signature created')
      setEditing(null)
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to save signature')
    } finally {
      setSaving(false)
    }
  }

  const handleDelete = async (id: number) => {
    try {
      await deleteJson(`/mail/signatures/${id}`)
      toast.success('Signature deleted')
      setDeleteTarget(null)
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete signature')
      setDeleteTarget(null)
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <SectionHeader title="Signatures" />
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
          Add Signature
        </button>
      </div>

      {editing && (
        <div className={cardClass + ' space-y-3'}>
          <input
            aria-label="Signature name"
            autoFocus
            className={inputClass}
            onChange={(e) => setEditing({ ...editing, name: e.target.value })}
            placeholder="Signature name"
            value={editing.name ?? ''}
          />
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">Text content</label>
            <textarea
              aria-label="Text content"
              className={inputClass}
              onChange={(e) => setEditing({ ...editing, text_content: e.target.value })}
              rows={3}
              value={editing.text_content ?? ''}
            />
          </div>
          <div>
            <label className="text-fg-secondary mb-1 block text-xs font-medium">HTML content</label>
            <textarea
              aria-label="HTML content"
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
            Default signature
          </label>
          <div className="flex gap-2">
            <button className={btnPrimary} disabled={saving} onClick={handleSave}>
              {saving ? 'Saving...' : 'Save'}
            </button>
            <button className={btnSecondary} onClick={() => setEditing(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {signatures.length === 0 && !editing && (
        <p className="text-fg-muted text-sm">No signatures configured</p>
      )}

      {signatures.map((sig) => (
        <div className={cardClass} key={sig.id}>
          <div className="flex items-start justify-between">
            <div>
              <span className="text-sm font-medium">{sig.name}</span>
              {sig.is_default && (
                <span className="bg-success/10 text-success ml-2 rounded-full px-2 py-0.5 text-xs">
                  Default
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
                Edit
              </button>
              <button className="text-danger text-xs" onClick={() => setDeleteTarget(sig.id)}>
                Delete
              </button>
            </div>
          </div>
        </div>
      ))}

      {deleteTarget !== null && (
        <ConfirmDialog
          message="Delete this signature? This cannot be undone."
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
        />
      )}
    </div>
  )
}
