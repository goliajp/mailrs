import type { HashBody, ObservationBody } from '@/wire/schemas/screening'

import { Button, toast } from '@goliapkg/gds'
import { useState } from 'react'

import { wireAddScreeningHash, wireAddScreeningObservation } from '@/wire/endpoints/screening'

const INPUT =
  'border-border bg-bg-secondary rounded-md border px-3 py-1.5 text-sm focus:border-accent focus:outline-none'

/** Record a file as known bad or known good. */
export function HashForm({ onDone }: { onDone: () => void }) {
  const [sha256, setSha] = useState('')
  const [stance, setStance] = useState<'allow' | 'deny'>('deny')
  const [reason, setReason] = useState('')
  const save = async () => {
    const body: HashBody = { reason, sha256: sha256.trim(), stance }
    try {
      await wireAddScreeningHash(body)
      toast.success('Recorded')
      setSha('')
      setReason('')
      onDone()
    } catch (e) {
      toast.error(errorText(e))
    }
  }
  return (
    <form
      className="flex flex-wrap items-end gap-2 text-sm"
      onSubmit={(e) => {
        e.preventDefault()
        void save()
      }}
    >
      <select
        aria-label="Hash stance"
        className={INPUT}
        onChange={(e) => setStance(e.target.value as 'allow' | 'deny')}
        value={stance}
      >
        <option value="deny">Known bad</option>
        <option value="allow">Known good</option>
      </select>
      <input
        aria-label="SHA-256"
        className={`${INPUT} w-96 max-w-full font-mono`}
        onChange={(e) => setSha(e.target.value)}
        placeholder="sha256 of the file"
        value={sha256}
      />
      <input
        aria-label="Hash reason"
        className={`${INPUT} min-w-64 flex-1`}
        onChange={(e) => setReason(e.target.value)}
        placeholder="Why"
        value={reason}
      />
      <Button disabled={sha256.trim().length !== 64 || !reason.trim()} size="sm" type="submit">
        Record
      </Button>
    </form>
  )
}

/** Record an incident or a manual allow or deny. */
export function ObservationForm({ onDone }: { onDone: () => void }) {
  const [source, setSource] = useState<ObservationBody['source']>('incident')
  const [kind, setKind] = useState<ObservationBody['kind']>('ip')
  const [stance, setStance] = useState<'allow' | 'deny'>('deny')
  const [value, setValue] = useState('')
  const [reason, setReason] = useState('')
  const save = async () => {
    try {
      await wireAddScreeningObservation({ kind, reason, source, stance, value })
      toast.success('Recorded')
      setValue('')
      setReason('')
      onDone()
    } catch (e) {
      toast.error(errorText(e))
    }
  }
  return (
    <form
      className="flex flex-wrap items-end gap-2 text-sm"
      onSubmit={(e) => {
        e.preventDefault()
        void save()
      }}
    >
      <select
        aria-label="Source"
        className={INPUT}
        onChange={(e) => setSource(e.target.value as ObservationBody['source'])}
        value={source}
      >
        <option value="incident">Incident</option>
        <option value="manual">Manual</option>
      </select>
      <select
        aria-label="Stance"
        className={INPUT}
        onChange={(e) => setStance(e.target.value as 'allow' | 'deny')}
        value={stance}
      >
        <option value="deny">Deny</option>
        <option value="allow">Allow</option>
      </select>
      <select
        aria-label="Kind"
        className={INPUT}
        onChange={(e) => setKind(e.target.value as ObservationBody['kind'])}
        value={kind}
      >
        <option value="ip">Address</option>
        <option value="cidr">Network</option>
        <option value="sender_domain">Sender domain</option>
        <option value="link_domain">Link domain</option>
      </select>
      <input
        aria-label="Value"
        className={`${INPUT} w-56 font-mono`}
        onChange={(e) => setValue(e.target.value)}
        placeholder="203.0.113.9"
        value={value}
      />
      <input
        aria-label="Reason"
        className={`${INPUT} min-w-64 flex-1`}
        onChange={(e) => setReason(e.target.value)}
        placeholder="Why, for whoever reads this later"
        value={reason}
      />
      <Button disabled={!value.trim() || !reason.trim()} size="sm" type="submit">
        Record
      </Button>
    </form>
  )
}

function errorText(e: unknown): string {
  if (e instanceof Error) return e.message
  return 'Something went wrong'
}
