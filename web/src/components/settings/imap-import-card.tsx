import type { WireImapImportStatus } from '@/wire/schemas/imap-import'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { invalidateMail } from '@/lib/mail-cache'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  wireCancelImapImport,
  wireImapImportStatus,
  wireStartImapImport,
} from '@/wire/endpoints/imap-import'

import { btnPrimary, btnSecondary, cardClass, Field, inputClass } from './_shared'

const EMPTY_FORM = { host: '', password: '', port: 993, username: '' }

type Form = typeof EMPTY_FORM

/** Bring mail over from another provider's IMAP server. */
export function ImapImportCard() {
  const t = useT()
  const [form, setForm] = useState(EMPTY_FORM)
  const [starting, setStarting] = useState(false)
  const query = useQuery({
    queryKey: settingsKeys.imapImport(),
    queryFn: ({ signal }) => wireImapImportStatus(signal),
    refetchInterval: (q) => {
      if (inProgress(q.state.data)) return 2000
      return false
    },
  })
  const status = query.data ?? null

  const start = async () => {
    setStarting(true)
    try {
      await wireStartImapImport(form)
      setForm({ ...form, password: '' })
      await queryClient.invalidateQueries({ queryKey: settingsKeys.imapImport() })
    } catch (e) {
      toast.error(errorText(e, t('settings.account.imapStartFailed')))
    } finally {
      setStarting(false)
    }
  }

  const cancel = async () => {
    await wireCancelImapImport()
    await queryClient.invalidateQueries({ queryKey: settingsKeys.imapImport() })
  }

  const finished = status?.state === 'done'
  useEffect(() => {
    if (finished) invalidateMail()
  }, [finished])

  let body = (
    <ImapForm
      form={form}
      onChange={setForm}
      onStart={() => void start()}
      starting={starting}
      status={status}
    />
  )
  if (status && inProgress(status)) {
    body = <ImapProgress onCancel={() => void cancel()} status={status} />
  }

  return (
    <div className={cardClass}>
      <h3 className="mb-2 text-sm font-medium">{t('settings.account.imapImport')}</h3>
      <p className="text-fg-muted mb-3 text-xs">{t('settings.account.imapHelp')}</p>
      {body}
    </div>
  )
}

function ImapForm({
  form,
  onChange,
  onStart,
  starting,
  status,
}: {
  form: Form
  onChange: (f: Form) => void
  onStart: () => void
  starting: boolean
  status: null | WireImapImportStatus
}) {
  const t = useT()
  return (
    <div className="space-y-2">
      {status && <ImapOutcome status={status} />}
      <Field label={t('settings.account.imapHost')}>
        <input
          aria-label={t('settings.account.imapHost')}
          className={inputClass}
          onChange={(e) => onChange({ ...form, host: e.target.value })}
          placeholder="imap.gmail.com"
          value={form.host}
        />
      </Field>
      <Field label={t('settings.account.imapPort')}>
        <select
          aria-label={t('settings.account.imapPort')}
          className={inputClass}
          onChange={(e) => onChange({ ...form, port: Number(e.target.value) })}
          value={form.port}
        >
          <option value={993}>{t('settings.account.imapPortTls')}</option>
          <option value={143}>{t('settings.account.imapPortStarttls')}</option>
        </select>
      </Field>
      <Field label={t('settings.account.imapUsername')}>
        <input
          aria-label={t('settings.account.imapUsername')}
          autoComplete="off"
          className={inputClass}
          onChange={(e) => onChange({ ...form, username: e.target.value })}
          value={form.username}
        />
      </Field>
      <Field label={t('settings.account.imapPassword')}>
        <input
          aria-label={t('settings.account.imapPassword')}
          autoComplete="new-password"
          className={inputClass}
          onChange={(e) => onChange({ ...form, password: e.target.value })}
          type="password"
          value={form.password}
        />
      </Field>
      <button
        className={btnPrimary}
        disabled={starting || !form.host || !form.username || !form.password}
        onClick={onStart}
      >
        {t('settings.account.imapStart')}
      </button>
    </div>
  )
}

function ImapOutcome({ status }: { status: WireImapImportStatus }) {
  const t = useT()
  const r = status.report
  if (status.state === 'none') return null
  if (status.state === 'interrupted') {
    return (
      <p className="text-xs" role="status">
        {t('settings.account.imapInterrupted', { host: status.host })}
      </p>
    )
  }
  return (
    <div className="text-fg-secondary space-y-1 text-xs" role="status">
      {status.state === 'done' && <p>{t('settings.account.imapDone', { host: status.host })}</p>}
      {status.state === 'cancelled' && <p>{t('settings.account.imapCancelled')}</p>}
      {status.state === 'failed' && (
        <p className="text-red-400">
          {t('settings.account.imapFailed', { reason: status.error ?? '' })}
        </p>
      )}
      <p>
        {t('settings.account.importDone', {
          archived: r.archived,
          imported: r.imported,
          sent: r.sent,
        })}
      </p>
      {r.held > 0 && <p>{t('settings.account.importHeld', { held: r.held })}</p>}
      <p>
        {t('settings.account.importSkipped', {
          duplicate: r.skipped_duplicate,
          junk: r.skipped_junk_or_trash,
        })}
      </p>
    </div>
  )
}

function ImapProgress({
  onCancel,
  status,
}: {
  onCancel: () => void
  status: WireImapImportStatus
}) {
  const t = useT()
  const r = status.report
  return (
    <div className="space-y-2 text-xs" role="status">
      <p>
        {status.state === 'queued' && t('settings.account.imapQueued', { host: status.host })}
        {status.state === 'running' &&
          t('settings.account.imapRunning', { folder: status.folder ?? '', host: status.host })}
      </p>
      <p className="text-fg-secondary">
        {t('settings.account.importDone', {
          archived: r.archived,
          imported: r.imported,
          sent: r.sent,
        })}
      </p>
      <button className={btnSecondary} onClick={onCancel}>
        {t('settings.account.imapCancel')}
      </button>
    </div>
  )
}

function inProgress(s: null | undefined | WireImapImportStatus) {
  return s?.state === 'queued' || s?.state === 'running'
}
