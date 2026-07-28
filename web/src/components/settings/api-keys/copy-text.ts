import { toast } from '@goliapkg/gds'

/**
 * Writes to the clipboard and reports the outcome. Resolves `false` when
 * the browser refuses (insecure origin, denied permission) so callers can
 * skip their "copied" confirmation instead of lying about it.
 */
export async function copyText(value: string, label: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value)
    toast.success(`${label} copied`)
    return true
  } catch {
    toast.error(`Could not copy ${label}`)
    return false
  }
}
