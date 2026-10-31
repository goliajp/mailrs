/** A short name for a user agent, or the agent itself. */
export function device(ua: string): string {
  if (!ua) return 'Unknown device'
  const browser = [
    ['Edg/', 'Edge'],
    ['Firefox/', 'Firefox'],
    ['Chrome/', 'Chrome'],
    ['Safari/', 'Safari'],
    ['mailrs-ios', 'mailrs for iOS'],
    ['mailrs-android', 'mailrs for Android'],
  ].find(([needle]) => ua.includes(needle))
  const system = [
    ['iPhone', 'iPhone'],
    ['iPad', 'iPad'],
    ['Android', 'Android'],
    ['Mac OS X', 'macOS'],
    ['Macintosh', 'macOS'],
    ['Windows', 'Windows'],
    ['Linux', 'Linux'],
  ].find(([needle]) => ua.includes(needle))
  if (!browser && !system) return ua.slice(0, 60)
  return [browser?.[1], system?.[1]].filter(Boolean).join(' on ')
}
