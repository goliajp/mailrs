/** Whom to ask for a reset code, as the server names them. */
export type ResetContact = { address: string; name: string }

/** What the operator types to clear `n` accounts. */
export function confirmPhrase(n: number): string {
  if (n === 1) return 'clear 1 account'
  return `clear ${n} accounts`
}

/** Whom to ask, as a sentence part: "Li Hao (lihao@golia.jp)", or `anyone` when nobody is named. */
export function contactText(contact: null | ResetContact, anyone: string): string {
  if (!contact) return anyone
  const { address, name } = contact
  if (name && address) return `${name} (${address})`
  return name || address || anyone
}

/** The addresses typed, one per line or separated by commas, once each. */
export function parseAddresses(text: string): string[] {
  const seen = new Set<string>()
  for (const part of text.split(/[\s,;]+/)) {
    const a = part.trim().toLowerCase()
    if (a) seen.add(a)
  }
  return [...seen]
}
