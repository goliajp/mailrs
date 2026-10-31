import { toast } from '@goliapkg/gds'
import { getDefaultStore } from 'jotai'

import { langAtom, translate } from '@/i18n'

/**
 * Writes to the clipboard and reports the outcome. Resolves `false` when
 * the browser refuses (insecure origin, denied permission) so callers can
 * skip their "copied" confirmation instead of lying about it.
 */
export async function copyText(value: string, label: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value)
    toast.success(translate(getDefaultStore().get(langAtom), 'settings.copiedNamed', { label }))
    return true
  } catch {
    toast.error(translate(getDefaultStore().get(langAtom), 'settings.copyFailedNamed', { label }))
    return false
  }
}
