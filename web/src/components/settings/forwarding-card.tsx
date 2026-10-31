import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { queryClient } from '@/lib/query-client'
import {
  wireGetForwarding,
  wireRequestForwarding,
  wireStopForwarding,
  wireVerifyForwarding,
} from '@/wire/endpoints/mail-settings'

import { btnPrimary, btnSecondary, cardClass, inputClass } from './_shared'
import { errorText } from './rules-shared'

const forwardingKey = ['settings', 'forwarding'] as const

/**
 * Forwarding to an address elsewhere. A new address asks for the password,
 * then for the code mailed to it: nothing is forwarded until the owner of
 * that address has typed it back.
 */
export function ForwardingCard() {
  const { data } = useQuery({
    queryKey: forwardingKey,
    queryFn: ({ signal }) => wireGetForwarding(signal),
  })
  const [address, setAddress] = useState('')
  const [keepCopy, setKeepCopy] = useState(true)
  const [code, setCode] = useState('')
  const refresh = () => queryClient.invalidateQueries({ queryKey: forwardingKey })
  const run = async (work: () => Promise<unknown>, done: string) => {
    try {
      await work()
      toast.success(done)
    } catch (e) {
      toast.error(errorText(e))
    } finally {
      void refresh()
    }
  }
  const on = data?.address ?? ''
  const pending = data?.pending_address ?? ''
  return (
    <div className={`${cardClass} space-y-3`}>
      <h3 className="text-sm font-medium">Forwarding</h3>
      {on && (
        <div className="flex items-center justify-between gap-3 text-sm">
          <span>
            Forwarding to <b>{on}</b>
            {!data?.keep_copy && ', without keeping a copy here'}
          </span>
          <button
            className={btnSecondary}
            onClick={() => run(wireStopForwarding, 'Forwarding stopped')}
            type="button"
          >
            Stop
          </button>
        </div>
      )}
      {pending && (
        <form
          className="space-y-2 text-sm"
          onSubmit={(e) => {
            e.preventDefault()
            void run(() => wireVerifyForwarding(code.trim()), 'Forwarding is on')
            setCode('')
          }}
        >
          <p>
            A code was sent to <b>{pending}</b>. Enter it here to start forwarding there.
          </p>
          <div className="flex gap-2">
            <input
              aria-label="Code"
              className={inputClass}
              inputMode="numeric"
              onChange={(e) => setCode(e.target.value)}
              value={code}
            />
            <button className={btnPrimary} disabled={code.trim().length === 0} type="submit">
              Confirm
            </button>
          </div>
        </form>
      )}
      <form
        className="space-y-2 text-sm"
        onSubmit={(e) => {
          e.preventDefault()
          void run(
            () => wireRequestForwarding(address.trim(), keepCopy),
            `A code was sent to ${address.trim()}`
          )
          setAddress('')
        }}
      >
        <label className="block space-y-1">
          <span className="text-fg-secondary">Forward my mail to</span>
          <input
            className={inputClass}
            onChange={(e) => setAddress(e.target.value)}
            placeholder="me@elsewhere.example"
            type="email"
            value={address}
          />
        </label>
        <label className="inline-flex items-center gap-1.5">
          <input
            checked={keepCopy}
            onChange={(e) => setKeepCopy(e.target.checked)}
            type="checkbox"
          />
          Keep a copy here
        </label>
        <div className="flex justify-end">
          <button className={btnPrimary} disabled={!address.includes('@')} type="submit">
            Send code
          </button>
        </div>
      </form>
      <p className="text-fg-muted text-xs">
        Mail in Junk or held for review is never forwarded. Your password is asked for before a new
        address is set.
      </p>
    </div>
  )
}
