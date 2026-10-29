import { wireConfirmStatus } from '@/wire/endpoints/auth'

import { askToConfirm } from './confirm-asker'

/**
 * Make sure this session confirmed its password recently, asking if not.
 *
 * For an operation reached by navigation rather than through `http` — a
 * download opened in a new tab — whose refusal would land as a bare error
 * page instead of the dialog.
 */
export async function ensureConfirmed(): Promise<boolean> {
  const status = await wireConfirmStatus()
  if (status.confirmed) return true
  return askToConfirm()
}
