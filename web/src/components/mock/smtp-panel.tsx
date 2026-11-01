import type { MockSmtpCredentials } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { KeyRound } from 'lucide-react'
import { useState } from 'react'

import { CopyButton } from '@/components/settings/api-keys/copy-button'
import { type Key, useT } from '@/i18n'
import { mockKeys } from '@/lib/query-keys'
import { wireRotateMockSmtpToken } from '@/wire/endpoints/mock'

import { btnPrimary } from '../settings/_shared'
import { errorText } from './errors'

const SUBMISSION_PORT = '587'

/**
 * Sandbox SMTP credentials. The password is only in the POST response, so
 * it is kept here until the panel switches namespace, and never again.
 */
export function SmtpPanel({ enabled, namespace }: { enabled: boolean; namespace: string }) {
  const t = useT()
  const queryClient = useQueryClient()
  const [issued, setIssued] = useState<null | { creds: MockSmtpCredentials; ns: string }>(null)

  const rotate = useMutation({
    mutationFn: () => wireRotateMockSmtpToken(namespace),
    onError: (e) => toast.error(errorText(e, t('mock.smtp.issueFailed'))),
    onSuccess: (creds) => {
      setIssued({ creds, ns: namespace })
      // `smtp_enabled` on the namespace flips with the first password
      void queryClient.invalidateQueries({ queryKey: mockKeys.namespaces() })
    },
  })

  const creds = credsFor(issued, namespace)

  return (
    <section aria-label={t('mock.tool.smtp')} className="space-y-3">
      <p className="text-fg-secondary text-xs">{t('mock.smtp.help')}</p>
      <div className="flex flex-wrap items-center gap-3">
        <button
          className={`${btnPrimary} inline-flex items-center gap-1.5`}
          disabled={rotate.isPending}
          onClick={() => rotate.mutate()}
          type="button"
        >
          <KeyRound aria-hidden className="h-4 w-4" />
          {t(issueLabel(enabled))}
        </button>
        <span className="text-fg-muted text-xs">{t(statusLabel(enabled))}</span>
      </div>
      {creds && (
        <dl className="border-warning/60 bg-warning/5 grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1.5 rounded-lg border px-3 py-2.5 text-sm">
          <CredRow
            label={t('mock.smtp.host')}
            value={hostLabel(creds.host, t('mock.smtp.noHost'))}
          />
          <CredRow
            copyValue={SUBMISSION_PORT}
            label={t('mock.smtp.port')}
            value={`${SUBMISSION_PORT} (STARTTLS)`}
          />
          <CredRow label={t('admin.username')} value={creds.username} />
          <CredRow label={t('admin.password')} value={creds.password} />
          <p className="text-fg-secondary col-span-3 text-xs">{t('mock.smtp.shownOnce')}</p>
        </dl>
      )}
    </section>
  )
}

function CredRow({
  copyValue,
  label,
  value,
}: {
  copyValue?: string
  label: string
  value: string
}) {
  return (
    <>
      <dt className="text-fg-muted text-xs">{label}</dt>
      <dd className="min-w-0 truncate font-mono">{value}</dd>
      <CopyButton label={label} value={copyValue ?? value} />
    </>
  )
}

function credsFor(
  issued: null | { creds: MockSmtpCredentials; ns: string },
  namespace: string
): MockSmtpCredentials | null {
  if (issued === null || issued.ns !== namespace) return null
  return issued.creds
}

/** `host` is MAILRS_HOSTNAME on the server and comes back empty when that is unset. */
function hostLabel(host: string, notConfigured: string): string {
  if (host === '') return notConfigured
  return host
}

function issueLabel(enabled: boolean): Key {
  if (enabled) return 'mock.smtp.issueNew'
  return 'mock.smtp.issue'
}

function statusLabel(enabled: boolean): Key {
  if (enabled) return 'mock.smtp.active'
  return 'mock.smtp.none'
}
