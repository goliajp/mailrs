import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { type Key, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
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
  const t = useT()
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
      toast.success(t('settings.keys.saved', { kind: type.toUpperCase() }))
      if (type === 'pgp') {
        setPgpKey('')
      } else {
        setSmimeCert('')
      }
      void invalidate()
    } catch (e) {
      toast.error(errorText(e, t('settings.keys.saveFailed', { kind: type })))
    } finally {
      setSaving(null)
    }
  }

  const handleDelete = async (type: 'pgp' | 'smime') => {
    setSaving(type)
    try {
      await wireDeleteKey(type)
      toast.success(t('settings.keys.deleted', { kind: type.toUpperCase() }))
      void invalidate()
    } catch (e) {
      toast.error(errorText(e, t('settings.keys.deleteFailed', { kind: type })))
    } finally {
      setSaving(null)
    }
  }

  return (
    <div className="space-y-6">
      <SectionHeader title={t('settings.keys.title')} />

      <div className={cardClass + ' space-y-4'}>
        <h3 className="text-sm font-medium">{t('settings.keys.pgp')}</h3>
        <Field label={t('settings.keys.fingerprint')}>
          <span className="text-fg-secondary font-mono text-xs">
            {status?.pgp_fingerprint ?? t('settings.account.notConfigured')}
          </span>
        </Field>
        <textarea
          aria-label={t('settings.keys.pgpPublic')}
          className={inputClass}
          onChange={(e) => setPgpKey(e.target.value)}
          placeholder={t('settings.keys.pgpPlaceholder')}
          rows={4}
          value={pgpKey}
        />
        <div className="flex gap-2">
          <button
            className={btnPrimary}
            disabled={saving === 'pgp'}
            onClick={() => handleUpload('pgp', pgpKey)}
          >
            {t(saveLabel(saving === 'pgp', 'settings.keys.savePgp'))}
          </button>
          {status?.pgp_fingerprint && (
            <button
              className={btnDanger}
              disabled={saving === 'pgp'}
              onClick={() => setDeleteKeyTarget('pgp')}
            >
              {t('settings.rules.delete')}
            </button>
          )}
        </div>
      </div>

      <div className={cardClass + ' space-y-4'}>
        <h3 className="text-sm font-medium">{t('settings.keys.smime')}</h3>
        <Field label={t('settings.keys.fingerprint')}>
          <span className="text-fg-secondary font-mono text-xs">
            {status?.smime_fingerprint ?? t('settings.account.notConfigured')}
          </span>
        </Field>
        <textarea
          aria-label={t('settings.keys.smimeCert')}
          className={inputClass}
          onChange={(e) => setSmimeCert(e.target.value)}
          placeholder={t('settings.keys.smimePlaceholder')}
          rows={4}
          value={smimeCert}
        />
        <div className="flex gap-2">
          <button
            className={btnPrimary}
            disabled={saving === 'smime'}
            onClick={() => handleUpload('smime', smimeCert)}
          >
            {t(saveLabel(saving === 'smime', 'settings.keys.saveSmime'))}
          </button>
          {status?.smime_fingerprint && (
            <button
              className={btnDanger}
              disabled={saving === 'smime'}
              onClick={() => setDeleteKeyTarget('smime')}
            >
              {t('settings.rules.delete')}
            </button>
          )}
        </div>
      </div>

      {deleteKeyTarget && (
        <ConfirmDialog
          message={t('settings.keys.deleteConfirm', { kind: deleteKeyTarget.toUpperCase() })}
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

function saveLabel(saving: boolean, idle: Key): Key {
  if (saving) return 'settings.saving'
  return idle
}
