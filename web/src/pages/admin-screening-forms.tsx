import type { HashBody, ObservationBody } from '@/wire/schemas/screening'

import { Button, toast } from '@goliapkg/gds'
import { useState } from 'react'

import { useT } from '@/i18n'
import { wireAddScreeningHash, wireAddScreeningObservation } from '@/wire/endpoints/screening'

const INPUT =
  'border-border bg-bg-secondary rounded-md border px-3 py-1.5 text-sm focus:border-accent focus:outline-none'

/** Record a file as known bad or known good. */
export function HashForm({ onDone }: { onDone: () => void }) {
  const t = useT()
  const [sha256, setSha] = useState('')
  const [stance, setStance] = useState<'allow' | 'deny'>('deny')
  const [reason, setReason] = useState('')
  const save = async () => {
    const body: HashBody = { reason, sha256: sha256.trim(), stance }
    try {
      await wireAddScreeningHash(body)
      toast.success(t('admin.screening.recorded'))
      setSha('')
      setReason('')
      onDone()
    } catch (e) {
      toast.error(errorText(e, t('admin.screening.failed')))
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
        aria-label={t('admin.screening.hashStance')}
        className={INPUT}
        onChange={(e) => setStance(e.target.value as 'allow' | 'deny')}
        value={stance}
      >
        <option value="deny">{t('admin.screening.knownBad')}</option>
        <option value="allow">{t('admin.screening.knownGood')}</option>
      </select>
      <input
        aria-label="SHA-256"
        className={`${INPUT} w-96 max-w-full font-mono`}
        onChange={(e) => setSha(e.target.value)}
        placeholder={t('admin.screening.shaPlaceholder')}
        value={sha256}
      />
      <input
        aria-label={t('admin.screening.hashReason')}
        className={`${INPUT} min-w-64 flex-1`}
        onChange={(e) => setReason(e.target.value)}
        placeholder={t('admin.screening.why')}
        value={reason}
      />
      <Button disabled={sha256.trim().length !== 64 || !reason.trim()} size="sm" type="submit">
        {t('admin.screening.record')}
      </Button>
    </form>
  )
}

/** Record an incident or a manual allow or deny. */
export function ObservationForm({ onDone }: { onDone: () => void }) {
  const t = useT()
  const [source, setSource] = useState<ObservationBody['source']>('incident')
  const [kind, setKind] = useState<ObservationBody['kind']>('ip')
  const [stance, setStance] = useState<'allow' | 'deny'>('deny')
  const [value, setValue] = useState('')
  const [reason, setReason] = useState('')
  const save = async () => {
    try {
      await wireAddScreeningObservation({ kind, reason, source, stance, value })
      toast.success(t('admin.screening.recorded'))
      setValue('')
      setReason('')
      onDone()
    } catch (e) {
      toast.error(errorText(e, t('admin.screening.failed')))
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
        aria-label={t('admin.aliases.source')}
        className={INPUT}
        onChange={(e) => setSource(e.target.value as ObservationBody['source'])}
        value={source}
      >
        <option value="incident">{t('admin.screening.incident')}</option>
        <option value="manual">{t('admin.screening.manual')}</option>
      </select>
      <select
        aria-label={t('admin.screening.stance')}
        className={INPUT}
        onChange={(e) => setStance(e.target.value as 'allow' | 'deny')}
        value={stance}
      >
        <option value="deny">{t('admin.screening.deny')}</option>
        <option value="allow">{t('admin.screening.allow')}</option>
      </select>
      <select
        aria-label={t('admin.col.type')}
        className={INPUT}
        onChange={(e) => setKind(e.target.value as ObservationBody['kind'])}
        value={kind}
      >
        <option value="ip">{t('admin.col.address')}</option>
        <option value="cidr">{t('admin.screening.network')}</option>
        <option value="sender_domain">{t('admin.screening.senderDomain')}</option>
        <option value="link_domain">{t('admin.screening.linkDomain')}</option>
      </select>
      <input
        aria-label={t('admin.greylist.value')}
        className={`${INPUT} w-56 font-mono`}
        onChange={(e) => setValue(e.target.value)}
        placeholder="203.0.113.9"
        value={value}
      />
      <input
        aria-label={t('admin.screening.reason')}
        className={`${INPUT} min-w-64 flex-1`}
        onChange={(e) => setReason(e.target.value)}
        placeholder={t('admin.screening.reasonPlaceholder')}
        value={reason}
      />
      <Button disabled={!value.trim() || !reason.trim()} size="sm" type="submit">
        {t('admin.screening.record')}
      </Button>
    </form>
  )
}

function errorText(e: unknown, fallback: string): string {
  if (e instanceof Error) return e.message
  return fallback
}
