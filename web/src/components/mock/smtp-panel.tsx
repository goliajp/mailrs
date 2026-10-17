import type { MockSmtpCredentials } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { KeyRound } from 'lucide-react'
import { useState } from 'react'

import { CopyButton } from '@/components/settings/api-keys/copy-button'
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
  const queryClient = useQueryClient()
  const [issued, setIssued] = useState<null | { creds: MockSmtpCredentials; ns: string }>(null)

  const rotate = useMutation({
    mutationFn: () => wireRotateMockSmtpToken(namespace),
    onError: (e) => toast.error(errorText(e, 'Could not issue a password')),
    onSuccess: (creds) => {
      setIssued({ creds, ns: namespace })
      // `smtp_enabled` on the namespace flips with the first password
      void queryClient.invalidateQueries({ queryKey: mockKeys.namespaces() })
    },
  })

  const creds = credsFor(issued, namespace)

  return (
    <section aria-label="SMTP sandbox" className="space-y-3">
      <p className="text-fg-secondary text-xs">
        Point an app&apos;s SMTP settings here. Mail it sends to mock addresses arrives in those
        namespaces; mail to any other address is kept in this namespace as captured-out and never
        sent. Issuing a new password replaces the old one.
      </p>
      <div className="flex flex-wrap items-center gap-3">
        <button
          className={`${btnPrimary} inline-flex items-center gap-1.5`}
          disabled={rotate.isPending}
          onClick={() => rotate.mutate()}
          type="button"
        >
          <KeyRound aria-hidden className="h-4 w-4" />
          {issueLabel(enabled)}
        </button>
        <span className="text-fg-muted text-xs">{statusLabel(enabled)}</span>
      </div>
      {creds && (
        <dl className="border-warning/60 bg-warning/5 grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1.5 rounded-lg border px-3 py-2.5 text-sm">
          <CredRow label="Host" value={hostLabel(creds.host)} />
          <CredRow
            copyValue={SUBMISSION_PORT}
            label="Port"
            value={`${SUBMISSION_PORT} (STARTTLS)`}
          />
          <CredRow label="Username" value={creds.username} />
          <CredRow label="Password" value={creds.password} />
          <p className="text-fg-secondary col-span-3 text-xs">
            Shown once. Copy the password now; it cannot be shown again
          </p>
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
function hostLabel(host: string): string {
  if (host === '') return '(server hostname not configured)'
  return host
}

function issueLabel(enabled: boolean): string {
  if (enabled) return 'Issue new password'
  return 'Issue password'
}

function statusLabel(enabled: boolean): string {
  if (enabled) return 'A password is active for this namespace'
  return 'No password issued yet'
}
