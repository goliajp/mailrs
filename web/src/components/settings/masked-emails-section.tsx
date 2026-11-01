import type { WireMaskedEmail } from '@/wire/schemas/masked-email'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { useLocale, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { queryClient } from '@/lib/query-client'
import { settingsKeys } from '@/lib/query-keys'
import {
  type MaskedEmailState,
  wireCreateMaskedEmail,
  wireDeleteMaskedEmail,
  wireListMaskedEmails,
  wireSetMaskedEmailState,
} from '@/wire/endpoints/masked-email'

import {
  btnDanger,
  btnPrimary,
  btnSecondary,
  cardClass,
  ConfirmDialog,
  inputClass,
  SectionHeader,
} from './_shared'

const EMPTY: WireMaskedEmail[] = []

/**
 * Masked addresses: one random address per site, delivering here. A
 * disabled one refuses mail; a deleted one refuses it for good and is never
 * issued again.
 */
export function MaskedEmailsSection() {
  const t = useT()
  const query = useQuery({
    queryKey: settingsKeys.maskedEmails(),
    queryFn: ({ signal }) => wireListMaskedEmails(signal),
  })
  const masks = query.data ?? EMPTY
  return (
    <div className="space-y-6">
      <SectionHeader title={t('settings.cat.masked')} />
      <p className="text-fg-muted text-xs">{t('settings.masked.blurb')}</p>
      <CreateMask />
      <div className={cardClass}>
        {masks.length === 0 && (
          <p className="text-fg-muted text-sm">{t('settings.masked.empty')}</p>
        )}
        <ul className="divide-border divide-y">
          {masks.map((m) => (
            <MaskRow key={m.email} mask={m} />
          ))}
        </ul>
      </div>
    </div>
  )
}

function CreateMask() {
  const t = useT()
  const [site, setSite] = useState('')
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)

  const create = async () => {
    setBusy(true)
    try {
      const m = await wireCreateMaskedEmail({ description: note.trim(), forDomain: site.trim() })
      toast.success(t('settings.masked.created', { email: m.email }))
      setSite('')
      setNote('')
      await invalidate()
    } catch (e) {
      toast.error(errorText(e, t('settings.masked.failed')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className={cardClass}>
      <div className="grid gap-2 sm:grid-cols-[1fr_1fr_auto]">
        <input
          aria-label={t('settings.masked.site')}
          className={inputClass}
          onChange={(e) => setSite(e.target.value)}
          placeholder={t('settings.masked.sitePlaceholder')}
          value={site}
        />
        <input
          aria-label={t('settings.masked.note')}
          className={inputClass}
          onChange={(e) => setNote(e.target.value)}
          placeholder={t('settings.masked.note')}
          value={note}
        />
        <button className={btnPrimary} disabled={busy} onClick={() => void create()} type="button">
          {t('settings.masked.create')}
        </button>
      </div>
    </div>
  )
}

/** The state a toggle moves a mask to. */
function flipped(state: WireMaskedEmail['state']): MaskedEmailState {
  if (state === 'disabled') return 'enabled'
  return 'disabled'
}

function invalidate() {
  return queryClient.invalidateQueries({ queryKey: settingsKeys.maskedEmails() })
}

function MaskRow({ mask }: { mask: WireMaskedEmail }) {
  const t = useT()
  const locale = useLocale()
  const [busy, setBusy] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const off = mask.state === 'disabled'

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await work()
      await invalidate()
    } catch (e) {
      toast.error(errorText(e, t('settings.masked.failed')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <li className="flex flex-wrap items-center justify-between gap-2 py-3">
      <div className="min-w-0">
        <p className="text-fg truncate font-mono text-sm">{mask.email}</p>
        <p className="text-fg-muted truncate text-xs">
          {[mask.forDomain, mask.description].filter((s) => s !== '').join(' · ')}
        </p>
        <p className="text-fg-muted text-xs">
          {mask.lastMessageAt !== null &&
            t('settings.masked.lastMessage', {
              date: new Date(mask.lastMessageAt * 1000).toLocaleDateString(locale),
            })}
          {mask.lastMessageAt === null && t('settings.masked.noMail')}
          {off && <> · {t('settings.masked.disabled')}</>}
        </p>
      </div>
      <div className="flex gap-2">
        <button
          className={btnSecondary}
          disabled={busy}
          onClick={() => void run(() => wireSetMaskedEmailState(mask.email, flipped(mask.state)))}
          type="button"
        >
          {t(toggleLabel(off))}
        </button>
        <button
          className={btnDanger}
          disabled={busy}
          onClick={() => setConfirming(true)}
          type="button"
        >
          {t('settings.masked.delete')}
        </button>
      </div>
      {confirming && (
        <ConfirmDialog
          message={t('settings.masked.confirmDelete', { email: mask.email })}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false)
            void run(() => wireDeleteMaskedEmail(mask.email))
          }}
        />
      )}
    </li>
  )
}

function toggleLabel(off: boolean) {
  if (off) return 'settings.masked.enable' as const
  return 'settings.masked.disable' as const
}
