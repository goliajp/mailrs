import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import { wireDeleteKey, wireGetKeyStatus, wireUploadKey } from '@/wire/endpoints/settings'

import {
  btnDanger,
  btnPrimary,
  cardClass,
  ConfirmDialog,
  Field,
  inputClass,
  SectionHeader,
} from './_shared'

export function EncryptionKeysSection() {
  const { data: status = null } = useQuery({
    queryKey: settingsKeys.encryptionKeysStatus(),
    queryFn: () => wireGetKeyStatus(),
  })
  const [pgpKey, setPgpKey] = useState('')
  const [smimeCert, setSmimeCert] = useState('')
  const [saving, setSaving] = useState<null | string>(null)
  const [deleteKeyTarget, setDeleteKeyTarget] = useState<'pgp' | 'smime' | null>(null)

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: settingsKeys.encryptionKeysStatus() })

  const handleUpload = async (type: 'pgp' | 'smime', content: string) => {
    if (!content.trim()) return
    setSaving(type)
    try {
      await wireUploadKey(type, content.trim())
      toast.success(`${type.toUpperCase()} key saved`)
      if (type === 'pgp') {
        setPgpKey('')
      } else {
        setSmimeCert('')
      }
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : `Failed to save ${type} key`)
    } finally {
      setSaving(null)
    }
  }

  const handleDelete = async (type: 'pgp' | 'smime') => {
    setSaving(type)
    try {
      await wireDeleteKey(type)
      toast.success(`${type.toUpperCase()} key deleted`)
      void invalidate()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : `Failed to delete ${type} key`)
    } finally {
      setSaving(null)
    }
  }

  return (
    <div className="space-y-6">
      <SectionHeader title="Encryption Keys" />

      <div className={cardClass + ' space-y-4'}>
        <h3 className="text-sm font-medium">PGP Key</h3>
        <Field label="Fingerprint">
          <span className="text-fg-secondary font-mono text-xs">
            {status?.pgp_fingerprint ?? 'Not configured'}
          </span>
        </Field>
        <textarea
          aria-label="PGP public key"
          className={inputClass}
          onChange={(e) => setPgpKey(e.target.value)}
          placeholder="Paste PGP public key (ASCII-armored)"
          rows={4}
          value={pgpKey}
        />
        <div className="flex gap-2">
          <button
            className={btnPrimary}
            disabled={saving === 'pgp'}
            onClick={() => handleUpload('pgp', pgpKey)}
          >
            {saving === 'pgp' ? 'Saving...' : 'Save PGP Key'}
          </button>
          {status?.pgp_fingerprint && (
            <button
              className={btnDanger}
              disabled={saving === 'pgp'}
              onClick={() => setDeleteKeyTarget('pgp')}
            >
              Delete
            </button>
          )}
        </div>
      </div>

      <div className={cardClass + ' space-y-4'}>
        <h3 className="text-sm font-medium">S/MIME Certificate</h3>
        <Field label="Fingerprint">
          <span className="text-fg-secondary font-mono text-xs">
            {status?.smime_fingerprint ?? 'Not configured'}
          </span>
        </Field>
        <textarea
          aria-label="S/MIME certificate"
          className={inputClass}
          onChange={(e) => setSmimeCert(e.target.value)}
          placeholder="Paste S/MIME certificate (PEM format)"
          rows={4}
          value={smimeCert}
        />
        <div className="flex gap-2">
          <button
            className={btnPrimary}
            disabled={saving === 'smime'}
            onClick={() => handleUpload('smime', smimeCert)}
          >
            {saving === 'smime' ? 'Saving...' : 'Save S/MIME Cert'}
          </button>
          {status?.smime_fingerprint && (
            <button
              className={btnDanger}
              disabled={saving === 'smime'}
              onClick={() => setDeleteKeyTarget('smime')}
            >
              Delete
            </button>
          )}
        </div>
      </div>

      {deleteKeyTarget && (
        <ConfirmDialog
          message={`Delete your ${deleteKeyTarget.toUpperCase()} key? This cannot be undone.`}
          onCancel={() => setDeleteKeyTarget(null)}
          onConfirm={() => {
            handleDelete(deleteKeyTarget)
            setDeleteKeyTarget(null)
          }}
        />
      )}
    </div>
  )
}
