import { toast } from '@goliapkg/gds'

import { tNow } from '@/i18n'

/**
 * Writes to the clipboard and reports the outcome. Resolves `false` when
 * the browser refuses (insecure origin, denied permission) so callers can
 * skip their "copied" confirmation instead of lying about it.
 */
export async function copyText(value: string, label: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value)
    toast.success(tNow('settings.copiedNamed', { label }))
    return true
  } catch {
    toast.error(tNow('settings.copyFailedNamed', { label }))
    return false
  }
}
