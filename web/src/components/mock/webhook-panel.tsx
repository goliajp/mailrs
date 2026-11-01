import type { MockWebhook } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { CopyButton } from '@/components/settings/api-keys/copy-button'
import { type Key, useT } from '@/i18n'
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
  const t = useT()
  const queryClient = useQueryClient()
  const [url, setUrl] = useState('')
  const [issued, setIssued] = useState<null | { hook: MockWebhook; ns: string }>(null)
  const refresh = () => void queryClient.invalidateQueries({ queryKey: mockKeys.namespaces() })

  const save = useMutation({
    mutationFn: () => wireSetMockWebhook(namespace, url.trim()),
    onError: (e) => toast.error(errorText(e, t('mock.hook.setFailed'))),
    onSuccess: (hook) => {
      setIssued({ hook, ns: namespace })
      setUrl('')
      refresh()
    },
  })
  const clear = useMutation({
    mutationFn: () => wireClearMockWebhook(namespace),
    onError: (e) => toast.error(errorText(e, t('mock.hook.removeFailed'))),
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
    <section aria-label={t('mock.tool.webhook')} className="space-y-3">
      <p className="text-fg-secondary text-xs">{t('mock.hook.help')}</p>
      <p className="text-fg-muted text-xs">{currentLabel(current, t)}</p>
      <form className="flex flex-wrap gap-2" onSubmit={submit}>
        <input
          aria-label={t('mock.hook.url')}
          className={`${inputClass} min-w-0 flex-1`}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://example.com/hooks/mock"
          value={url}
        />
        <button className={btnPrimary} disabled={save.isPending} type="submit">
          {t('admin.save')}
        </button>
        {current !== null && (
          <button
            className={btnSecondary}
            disabled={clear.isPending}
            onClick={() => clear.mutate()}
            type="button"
          >
            {t('admin.remove')}
          </button>
        )}
      </form>
      {hook && (
        <div className="border-warning/60 bg-warning/5 flex items-center gap-3 rounded-lg border px-3 py-2.5 text-sm">
          <span className="text-fg-muted text-xs">{t('mock.hook.secret')}</span>
          <span className="min-w-0 flex-1 truncate font-mono">{hook.signing_secret}</span>
          <CopyButton label={t('mock.hook.secret')} value={hook.signing_secret} />
        </div>
      )}
    </section>
  )
}

function currentLabel(
  current: null | string,
  t: (key: Key, vars?: Record<string, string>) => string
): string {
  if (current === null) return t('mock.hook.none')
  return t('mock.hook.posting', { url: current })
}

function hookFor(
  issued: null | { hook: MockWebhook; ns: string },
  namespace: string
): MockWebhook | null {
  if (issued === null || issued.ns !== namespace) return null
  return issued.hook
}
