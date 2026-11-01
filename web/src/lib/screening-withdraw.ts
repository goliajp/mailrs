import type { useT } from '@/i18n'

import { toast } from '@goliapkg/gds'

import { wireAddScreeningHash, wireAddScreeningObservation } from '@/wire/endpoints/screening'

/** Withdraw an observation: a new fact naming the old one. */
export async function withdraw(
  id: string,
  isHash: boolean,
  onDone: () => void,
  t: ReturnType<typeof useT>
): Promise<void> {
  const reason = window.prompt(t('admin.screening.withdrawWhy'))?.trim()
  if (!reason) return
  try {
    if (isHash) await wireAddScreeningHash({ reason, retracts: id, sha256: '', stance: 'retract' })
    else
      await wireAddScreeningObservation({
        kind: 'ip',
        reason,
        retracts: id,
        source: 'manual',
        stance: 'retract',
        value: '',
      })
    toast.success(t('admin.screening.withdrawn'))
    onDone()
  } catch (e) {
    toast.error(errorText(e, t('admin.screening.failed')))
  }
}

function errorText(e: unknown, fallback: string): string {
  if (e instanceof Error) return e.message
  return fallback
}
