/** A count, or a dash when it is not known. */
export function count(n: null | number): string {
  if (n === null) return '—'
  return n.toLocaleString('en-US')
}

/** `3h 4m`, `2d 1h`, `5m`. */
export function uptime(secs: number): string {
  const days = Math.floor(secs / 86400)
  const hours = Math.floor((secs % 86400) / 3600)
  const mins = Math.floor((secs % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h`
  if (hours > 0) return `${hours}h ${mins}m`
  return `${mins}m`
}
