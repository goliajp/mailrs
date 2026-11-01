import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { type Key, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { invalidateMail } from '@/lib/mail-cache'
import { queryClient } from '@/lib/query-client'
import {
  wireDeleteSenderRule,
  wireListSenderRules,
  wirePutSenderRule,
} from '@/wire/endpoints/sender-rules'
import { type SenderBucket, type WireSenderRule } from '@/wire/schemas/sender-rules'

import { btnPrimary, btnSecondary, cardClass, inputClass, SectionHeader } from './_shared'
import { type Draft, draftBody, EMPTY_DRAFT } from './notify-shared'

const RULES_KEY = ['settings', 'sender-rules'] as const

const BUCKETS: { key: Key; value: SenderBucket }[] = [
  { key: 'settings.senderRules.inbox', value: 'inbox' },
  { key: 'settings.senderRules.notifications', value: 'notifications' },
  { key: 'settings.senderRules.promotions', value: 'promotions' },
]

const DISPOSAL_LABEL: Record<WireSenderRule['disposal'], Key> = {
  archive: 'settings.senderRules.archive',
  trash: 'settings.senderRules.trash',
}

/** Where a sender's mail is filed, and how long it is kept. */
export function SenderRulesCard() {
  const t = useT()
  const summary = useSummary()
  const { data: rules = [] } = useQuery({ queryFn: wireListSenderRules, queryKey: RULES_KEY })
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT)
  const [busy, setBusy] = useState(false)
  const body = draftBody(draft)

  const save = async () => {
    if (!body) return
    setBusy(true)
    try {
      const answer = await wirePutSenderRule(draft.address.trim(), body)
      toast.success(t('settings.senderRules.saved', { count: answer.disposed }))
      setDraft(EMPTY_DRAFT)
      invalidateMail()
      void queryClient.invalidateQueries({ queryKey: RULES_KEY })
    } catch (e) {
      toast.error(errorText(e, t('settings.senderRules.failed')))
    } finally {
      setBusy(false)
    }
  }

  const remove = async (address: string) => {
    try {
      await wireDeleteSenderRule(address)
      void queryClient.invalidateQueries({ queryKey: RULES_KEY })
    } catch (e) {
      toast.error(errorText(e, t('settings.senderRules.failed')))
    }
  }

  return (
    <section className={cardClass}>
      <SectionHeader title={t('settings.senderRules.title')} />
      <p className="text-fg-muted mb-3 text-sm">{t('settings.senderRules.blurb')}</p>
      <div className="mb-4 grid gap-2 sm:grid-cols-2">
        <input
          aria-label={t('settings.senderRules.address')}
          className={inputClass}
          onChange={(e) => setDraft({ ...draft, address: e.target.value })}
          placeholder="news@example.com"
          value={draft.address}
        />
        <select
          aria-label={t('settings.senderRules.bucket')}
          className={inputClass}
          onChange={(e) => setDraft({ ...draft, bucket: e.target.value as Draft['bucket'] })}
          value={draft.bucket}
        >
          <option value="">{t('settings.senderRules.bucketNone')}</option>
          {BUCKETS.map((b) => (
            <option key={b.value} value={b.value}>
              {t(b.key)}
            </option>
          ))}
        </select>
        <input
          aria-label={t('settings.senderRules.keepLatest')}
          className={inputClass}
          inputMode="numeric"
          onChange={(e) => setDraft({ ...draft, keep: e.target.value })}
          placeholder={t('settings.senderRules.keepLatest')}
          value={draft.keep}
        />
        <input
          aria-label={t('settings.senderRules.olderThan')}
          className={inputClass}
          inputMode="numeric"
          onChange={(e) => setDraft({ ...draft, days: e.target.value })}
          placeholder={t('settings.senderRules.olderThan')}
          value={draft.days}
        />
        <select
          aria-label={t('settings.senderRules.disposal')}
          className={inputClass}
          onChange={(e) => setDraft({ ...draft, disposal: e.target.value as Draft['disposal'] })}
          value={draft.disposal}
        >
          <option value="archive">{t('settings.senderRules.archive')}</option>
          <option value="trash">{t('settings.senderRules.trash')}</option>
        </select>
        <button
          className={btnPrimary}
          disabled={busy || !body || !draft.address.includes('@')}
          onClick={() => void save()}
        >
          {t('settings.senderRules.save')}
        </button>
      </div>
      {rules.length === 0 && (
        <p className="text-fg-muted text-sm">{t('settings.senderRules.empty')}</p>
      )}
      <ul className="divide-border divide-y">
        {rules.map((r) => (
          <li className="flex items-center justify-between gap-2 py-2" key={r.address}>
            <span className="min-w-0 text-sm">
              <span className="text-fg block truncate">{r.address}</span>
              <span className="text-fg-muted block text-xs">{summary(r)}</span>
            </span>
            <button className={btnSecondary} onClick={() => void remove(r.address)}>
              {t('settings.senderRules.remove')}
            </button>
          </li>
        ))}
      </ul>
    </section>
  )
}

/** One line saying what a rule does. */
function useSummary(): (r: WireSenderRule) => string {
  const t = useT()
  return (r) => {
    const parts: string[] = []
    const bucket = BUCKETS.find((b) => b.value === r.bucket)
    if (bucket) parts.push(t(bucket.key))
    if (r.keep_latest !== undefined)
      parts.push(t('settings.senderRules.keepSummary', { n: r.keep_latest }))
    if (r.older_than_days !== undefined)
      parts.push(t('settings.senderRules.ageSummary', { days: r.older_than_days }))
    if (r.keep_latest !== undefined || r.older_than_days !== undefined) {
      parts.push(t(DISPOSAL_LABEL[r.disposal]))
    }
    return parts.join(' · ')
  }
}
