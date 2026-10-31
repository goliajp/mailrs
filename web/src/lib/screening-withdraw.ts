import { toast } from '@goliapkg/gds'

import { wireAddScreeningHash, wireAddScreeningObservation } from '@/wire/endpoints/screening'

/** Withdraw an observation: a new fact naming the old one. */
export async function withdraw(id: string, isHash: boolean, onDone: () => void): Promise<void> {
  const reason = window.prompt('Why is this withdrawn?')?.trim()
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
    toast.success('Withdrawn')
    onDone()
  } catch (e) {
    toast.error(errorText(e))
  }
}

function errorText(e: unknown): string {
  if (e instanceof Error) return e.message
  return 'Something went wrong'
}
