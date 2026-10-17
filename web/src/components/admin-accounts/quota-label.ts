/** Unknown is a dash, never `0` or `null B`; `-1` is the unlimited sentinel. */
export function quotaLabel(bytes: null | number | undefined): string {
  if (bytes === null || bytes === undefined) return '—'
  if (bytes < 0) return 'Unlimited'
  return formatBytes(bytes)
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}
