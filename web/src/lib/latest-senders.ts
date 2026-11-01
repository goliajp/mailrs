/**
 * The bare address of each conversation's latest correspondent: the last
 * participant who is not the reader, as the row lists them oldest first.
 */
export function latestSenders(rows: { participants: string[] }[], me: string): string[] {
  const out = new Set<string>()
  for (const row of rows) {
    const addresses = row.participants.map(bareAddress).filter((a) => a !== '' && a !== me)
    const last = addresses[addresses.length - 1]
    if (last) out.add(last)
  }
  return [...out]
}

function bareAddress(shown: string): string {
  const angled = /<([^>]+)>/.exec(shown)
  if (angled) return angled[1].trim().toLowerCase()
  return shown.trim().toLowerCase()
}
