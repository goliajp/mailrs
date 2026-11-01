import type { WirePasswordClearPreview } from '@/wire/schemas/admin'

import { Alert, Button } from '@goliapkg/gds'
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'

import { AdminPageShell, AdminTable, type Column } from '@/components/admin-page'
import { inputClass } from '@/components/settings/_shared'
import { type Key, useT } from '@/i18n'
import { confirmPhrase, parseAddresses } from '@/lib/password-clear'
import { wirePasswordClear, wirePasswordClearPreview } from '@/wire/endpoints/admin'

type Row = WirePasswordClearPreview['items'][number]

/** An account that runs a service: clearing it stops something. */
function runsSomething(r: Row): boolean {
  return r.agent_keys.length > 0 || r.smtp_password_logins_7d > 0
}

function stateClass(r: Row): string {
  if (!r.exists) return 'text-danger text-xs'
  if (r.cleared_at !== null) return 'text-fg-muted text-xs'
  return 'text-fg-secondary text-xs'
}

function stateLabel(r: Row): Key {
  if (!r.exists) return 'admin.clear.noAccount'
  if (r.cleared_at !== null) return 'admin.clear.already'
  return 'admin.clear.active'
}

function yesNo(v: boolean, t: ReturnType<typeof useT>): string {
  if (v) return t('admin.clear.yes')
  return '—'
}

const columns = (t: ReturnType<typeof useT>): Column<Row>[] => [
  {
    header: t('admin.signIns.account'),
    key: 'address',
    cell: (r) => <span className="font-medium break-all">{r.address}</span>,
  },
  {
    header: t('admin.col.status'),
    key: 'state',
    cell: (r) => <span className={stateClass(r)}>{t(stateLabel(r))}</span>,
  },
  { align: 'end', header: t('admin.clear.sessions'), key: 'sessions', cell: (r) => r.sessions },
  {
    header: t('admin.clear.agentKeys'),
    key: 'keys',
    cell: (r) => {
      if (r.agent_keys.length === 0) return '—'
      return (
        <span className="text-warning text-xs">
          {r.agent_keys.map((k) => `${k.name || t('admin.unnamed')} (${k.prefix}…)`).join(', ')}
        </span>
      )
    },
  },
  {
    align: 'end',
    header: t('admin.clear.smtpSignIns'),
    key: 'smtp',
    cell: (r) => {
      if (r.smtp_password_logins_7d === 0) return '—'
      return <span className="text-warning">{r.smtp_password_logins_7d}</span>
    },
  },
  { header: t('admin.clear.authenticator'), key: 'totp', cell: (r) => yesNo(r.totp, t) },
  {
    align: 'end',
    header: t('admin.clear.linked'),
    key: 'ext',
    cell: (r) => r.external_logins,
  },
]

/**
 * Clearing passwords: no password signs these accounts in until each owner
 * sets a new one with a code an issuer hands them. Their sessions end and
 * their open mail connections close; agent keys keep working.
 */
export function AdminPasswordClear() {
  const t = useT()
  const [text, setText] = useState('')
  const [typed, setTyped] = useState('')
  const preview = useMutation({ mutationFn: wirePasswordClearPreview })
  const clear = useMutation({ mutationFn: wirePasswordClear })

  const rows = preview.data?.items ?? []
  const clearable = rows.filter((r) => r.exists).map((r) => r.address)
  const phrase = confirmPhrase(clearable.length)

  const runPreview = () => {
    setTyped('')
    clear.reset()
    preview.mutate(parseAddresses(text))
  }

  return (
    <AdminPageShell subtitle={t('admin.clear.subtitle')} title={t('admin.clear.title')}>
      <div className="flex flex-col gap-4">
        <label className="flex flex-col gap-1">
          <span className="text-fg-secondary text-xs font-medium">
            {t('admin.clear.onePerLine')}
          </span>
          <textarea
            aria-label={t('admin.accounts.title')}
            className={inputClass}
            onChange={(e) => setText(e.target.value)}
            placeholder="someone@golia.jp"
            rows={5}
            value={text}
          />
        </label>
        <div>
          <Button
            disabled={parseAddresses(text).length === 0}
            loading={preview.isPending}
            onClick={runPreview}
            variant="secondary"
          >
            {t('admin.clear.preview')}
          </Button>
        </div>

        {preview.error && (
          <Alert role="alert" variant="danger">
            {preview.error.message}
          </Alert>
        )}

        {preview.data && (
          <>
            {!preview.data.issuers_configured && (
              <Alert role="alert" variant="warning">
                {t('admin.clear.noIssuers')}
              </Alert>
            )}
            {rows.some(runsSomething) && (
              <Alert role="status" variant="warning">
                {t('admin.clear.runsSomething')}
              </Alert>
            )}
            <AdminTable
              columns={columns(t)}
              label={t('admin.clear.toClear')}
              rowKey={(r) => r.address}
              rows={rows}
            />

            {clearable.length > 0 && !clear.data && (
              <div className="flex flex-col gap-2">
                <label className="flex flex-col gap-1">
                  <span className="text-fg-secondary text-xs font-medium">
                    {t('admin.clear.typeBefore')}
                    <span className="text-fg font-mono">{phrase}</span>
                    {t('admin.clear.typeAfter')}
                  </span>
                  <input
                    aria-label={t('admin.clear.confirmation')}
                    className={inputClass}
                    onChange={(e) => setTyped(e.target.value)}
                    value={typed}
                  />
                </label>
                <div>
                  <Button
                    disabled={typed.trim().toLowerCase() !== phrase}
                    loading={clear.isPending}
                    onClick={() => clear.mutate(clearable)}
                    variant="danger"
                  >
                    {t('admin.clear.submit', { n: clearable.length })}
                  </Button>
                </div>
              </div>
            )}
          </>
        )}

        {clear.error && (
          <Alert role="alert" variant="danger">
            {clear.error.message}
          </Alert>
        )}
        {clear.data && (
          <Alert role="status" variant="success">
            {t('admin.clear.done', {
              accounts: clear.data.items.filter((i) => i.result === 'cleared').length,
              sessions: clear.data.items.reduce((n, i) => n + i.sessions_revoked, 0),
            })}
          </Alert>
        )}
      </div>
    </AdminPageShell>
  )
}
