import type { WirePasswordClearPreview } from '@/wire/schemas/admin'

import { Alert, Button } from '@goliapkg/gds'
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'

import { AdminPageShell, AdminTable, type Column } from '@/components/admin-page'
import { inputClass } from '@/components/settings/_shared'
import { confirmPhrase, parseAddresses } from '@/lib/password-clear'
import { wirePasswordClear, wirePasswordClearPreview } from '@/wire/endpoints/admin'

type Row = WirePasswordClearPreview['items'][number]

/** An account that runs a service: clearing it stops something. */
function runsSomething(r: Row): boolean {
  return r.agent_keys.length > 0 || r.smtp_password_logins_7d > 0
}

function yesNo(v: boolean): string {
  if (v) return 'yes'
  return '—'
}

const columns: Column<Row>[] = [
  {
    header: 'Account',
    key: 'address',
    cell: (r) => <span className="font-medium break-all">{r.address}</span>,
  },
  {
    header: 'State',
    key: 'state',
    cell: (r) => {
      if (!r.exists) return <span className="text-danger text-xs">no such account</span>
      if (r.cleared_at !== null)
        return <span className="text-fg-muted text-xs">already cleared</span>
      return <span className="text-fg-secondary text-xs">active</span>
    },
  },
  { align: 'end', header: 'Sessions', key: 'sessions', cell: (r) => r.sessions },
  {
    header: 'Agent keys (keep working)',
    key: 'keys',
    cell: (r) => {
      if (r.agent_keys.length === 0) return '—'
      return (
        <span className="text-warning text-xs">
          {r.agent_keys.map((k) => `${k.name || 'unnamed'} (${k.prefix}…)`).join(', ')}
        </span>
      )
    },
  },
  {
    align: 'end',
    header: 'SMTP password sign-ins, 7 days',
    key: 'smtp',
    cell: (r) => {
      if (r.smtp_password_logins_7d === 0) return '—'
      return <span className="text-warning">{r.smtp_password_logins_7d}</span>
    },
  },
  { header: 'Authenticator', key: 'totp', cell: (r) => yesNo(r.totp) },
  { align: 'end', header: 'Linked sign-ins', key: 'ext', cell: (r) => r.external_logins },
]

/**
 * Clearing passwords: no password signs these accounts in until each owner
 * sets a new one with a code an issuer hands them. Their sessions end and
 * their open mail connections close; agent keys keep working.
 */
export function AdminPasswordClear() {
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
    <AdminPageShell
      subtitle="Nobody can sign in to a cleared account with a password until its owner sets a new one with a reset code. Its sessions end and its open mail connections close; agent keys keep working."
      title="Clear passwords"
    >
      <div className="flex flex-col gap-4">
        <label className="flex flex-col gap-1">
          <span className="text-fg-secondary text-xs font-medium">Accounts, one per line</span>
          <textarea
            aria-label="Accounts"
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
            Preview
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
                Nobody can hand out reset codes: MAILRS_RESET_ISSUERS is empty. A cleared account
                would have no way back in.
              </Alert>
            )}
            {rows.some(runsSomething) && (
              <Alert role="status" variant="warning">
                The highlighted accounts have agent keys or signed in to SMTP with their password in
                the last 7 days: something runs on them. Clearing stops the SMTP sign-ins; agent
                keys keep working.
              </Alert>
            )}
            <AdminTable
              columns={columns}
              label="Accounts to clear"
              rowKey={(r) => r.address}
              rows={rows}
            />

            {clearable.length > 0 && !clear.data && (
              <div className="flex flex-col gap-2">
                <label className="flex flex-col gap-1">
                  <span className="text-fg-secondary text-xs font-medium">
                    Type <span className="text-fg font-mono">{phrase}</span> to clear
                  </span>
                  <input
                    aria-label="Confirmation"
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
                    Clear {clearable.length} passwords
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
            Cleared {clear.data.items.filter((i) => i.result === 'cleared').length} accounts;{' '}
            {clear.data.items.reduce((n, i) => n + i.sessions_revoked, 0)} sessions ended.
          </Alert>
        )}
      </div>
    </AdminPageShell>
  )
}
