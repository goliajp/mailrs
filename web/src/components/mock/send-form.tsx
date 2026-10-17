import type { MockSendResult } from '@/wire/schemas/mock'

import { toast } from '@goliapkg/gds'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Send } from 'lucide-react'
import { useState } from 'react'

import { mockKeys } from '@/lib/query-keys'
import { wireMockSend } from '@/wire/endpoints/mock'

import { btnPrimary, inputClass } from '../settings/_shared'
import { errorText } from './errors'
import { splitRecipients } from './format'

/** Sends from `{ns}.sender@…`. Mock recipients receive it; everyone else is only recorded. */
export function SendForm({ namespace }: { namespace: string }) {
  const queryClient = useQueryClient()
  const [to, setTo] = useState('')
  const [subject, setSubject] = useState('')
  const [text, setText] = useState('')
  const [result, setResult] = useState<null | { ns: string; out: MockSendResult }>(null)

  const send = useMutation({
    mutationFn: () => wireMockSend(namespace, { subject, text, to: splitRecipients(to) }),
    onError: (e) => toast.error(errorText(e, 'Could not send')),
    onSuccess: (out) => {
      setResult({ ns: namespace, out })
      void queryClient.invalidateQueries({ queryKey: mockKeys.all() })
    },
  })

  const canSend = splitRecipients(to).length > 0 && !send.isPending

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!canSend) return
    send.mutate()
  }

  const shown = resultFor(result, namespace)

  return (
    <section aria-label="Send test mail" className="space-y-3">
      <form className="space-y-2" onSubmit={submit}>
        <input
          aria-label="To"
          className={inputClass}
          onChange={(e) => setTo(e.target.value)}
          placeholder="To: a.b@mock.example, c@example.com"
          value={to}
        />
        <input
          aria-label="Subject"
          className={inputClass}
          onChange={(e) => setSubject(e.target.value)}
          placeholder="Subject"
          value={subject}
        />
        <textarea
          aria-label="Text"
          className={`${inputClass} min-h-20`}
          onChange={(e) => setText(e.target.value)}
          placeholder="Text"
          value={text}
        />
        <button
          className={`${btnPrimary} inline-flex items-center gap-1.5`}
          disabled={!canSend}
          type="submit"
        >
          <Send aria-hidden className="h-4 w-4" />
          Send
        </button>
      </form>
      {shown && <SendOutcome result={shown} />}
    </section>
  )
}

function resultFor(
  result: null | { ns: string; out: MockSendResult },
  namespace: string
): MockSendResult | null {
  if (result === null || result.ns !== namespace) return null
  return result.out
}

function SendOutcome({ result }: { result: MockSendResult }) {
  return (
    <div className="border-border space-y-1 rounded-md border px-3 py-2 text-xs" role="status">
      {result.delivered.map((a) => (
        <p key={`ok-${a}`}>
          <span className="text-success font-medium">delivered</span> {a}
        </p>
      ))}
      {result.not_delivered.map((n) => (
        <p key={`no-${n.address}`}>
          <span className="text-warning font-medium">not delivered</span> {n.address}
          <span className="text-fg-muted"> — {n.reason}</span>
        </p>
      ))}
    </div>
  )
}
