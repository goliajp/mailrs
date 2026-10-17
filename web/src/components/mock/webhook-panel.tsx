import type { MockWebhook } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { CopyButton } from '@/components/settings/api-keys/copy-button'
import { mockKeys } from '@/lib/query-keys'
import { wireClearMockWebhook, wireSetMockWebhook } from '@/wire/endpoints/mock'

import { btnPrimary, btnSecondary, inputClass } from '../settings/_shared'
import { errorText } from './errors'

/**
 * Each message that arrives in the namespace is POSTed to this URL, signed
 * like every other webhook. The secret is only in the PUT response.
 */
export function WebhookPanel({
  current,
  namespace,
}: {
  current: null | string
  namespace: string
}) {
  const queryClient = useQueryClient()
  const [url, setUrl] = useState('')
  const [issued, setIssued] = useState<null | { hook: MockWebhook; ns: string }>(null)
  const refresh = () => void queryClient.invalidateQueries({ queryKey: mockKeys.namespaces() })

  const save = useMutation({
    mutationFn: () => wireSetMockWebhook(namespace, url.trim()),
    onError: (e) => toast.error(errorText(e, 'Could not set the webhook')),
    onSuccess: (hook) => {
      setIssued({ hook, ns: namespace })
      setUrl('')
      refresh()
    },
  })
  const clear = useMutation({
    mutationFn: () => wireClearMockWebhook(namespace),
    onError: (e) => toast.error(errorText(e, 'Could not remove the webhook')),
    onSuccess: () => {
      setIssued(null)
      refresh()
    },
  })

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    if (url.trim() === '' || save.isPending) return
    save.mutate()
  }
  const hook = hookFor(issued, namespace)

  return (
    <section aria-label="Webhook" className="space-y-3">
      <p className="text-fg-secondary text-xs">
        Each new message is POSTed as JSON with event &quot;mock_message&quot;, signed with the
        secret below. Setting a new URL replaces the old one and its secret.
      </p>
      <p className="text-fg-muted text-xs">{currentLabel(current)}</p>
      <form className="flex flex-wrap gap-2" onSubmit={submit}>
        <input
          aria-label="Webhook URL"
          className={`${inputClass} min-w-0 flex-1`}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://example.com/hooks/mock"
          value={url}
        />
        <button className={btnPrimary} disabled={save.isPending} type="submit">
          Save
        </button>
        {current !== null && (
          <button
            className={btnSecondary}
            disabled={clear.isPending}
            onClick={() => clear.mutate()}
            type="button"
          >
            Remove
          </button>
        )}
      </form>
      {hook && (
        <div className="border-warning/60 bg-warning/5 flex items-center gap-3 rounded-lg border px-3 py-2.5 text-sm">
          <span className="text-fg-muted text-xs">Signing secret</span>
          <span className="min-w-0 flex-1 truncate font-mono">{hook.signing_secret}</span>
          <CopyButton label="Signing secret" value={hook.signing_secret} />
        </div>
      )}
    </section>
  )
}

function currentLabel(current: null | string): string {
  if (current === null) return 'No webhook set'
  return `Posting to ${current}`
}

function hookFor(
  issued: null | { hook: MockWebhook; ns: string },
  namespace: string
): MockWebhook | null {
  if (issued === null || issued.ns !== namespace) return null
  return issued.hook
}
