// decode RFC 2047 encoded-words in email headers (e.g. =?UTF-8?B?...?= or =?UTF-8?Q?...?=)
export function decodeMimeHeader(value: string): string {
  if (!value.includes('=?')) return value
  return value.replace(
    /=\?([^?]+)\?(B|Q)\?([^?]*)\?=/gi,
    (_match, charset: string, encoding: string, encoded: string) => {
      try {
        if (encoding.toUpperCase() === 'B') {
          // base64
          const binary = atob(encoded)
          const bytes = new Uint8Array(binary.length)
          for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
          return new TextDecoder(charset).decode(bytes)
        }
        // quoted-printable
        const decoded = encoded
          .replace(/_/g, ' ')
          .replace(/=([0-9A-Fa-f]{2})/g, (_m, hex: string) => String.fromCharCode(parseInt(hex, 16)))
        const bytes = new Uint8Array(decoded.length)
        for (let i = 0; i < decoded.length; i++) bytes[i] = decoded.charCodeAt(i)
        return new TextDecoder(charset).decode(bytes)
      } catch {
        return encoded
      }
    },
  ).replace(/\s+/g, ' ').trim()
}

// generate a consistent color from an email address
const colors = [
  'bg-red-500',
  'bg-orange-500',
  'bg-amber-500',
  'bg-yellow-500',
  'bg-lime-500',
  'bg-green-500',
  'bg-emerald-500',
  'bg-teal-500',
  'bg-cyan-500',
  'bg-sky-500',
  'bg-blue-500',
  'bg-indigo-500',
  'bg-violet-500',
  'bg-purple-500',
  'bg-fuchsia-500',
  'bg-pink-500',
]

export function avatarColor(email: string): string {
  let hash = 0
  for (let i = 0; i < email.length; i++) {
    hash = (hash * 31 + email.charCodeAt(i)) | 0
  }
  return colors[Math.abs(hash) % colors.length]
}

export function avatarInitial(sender: string): string {
  const name = extractName(sender)
  return (name[0] ?? '?').toUpperCase()
}

export function extractEmail(sender: string): string {
  const decoded = decodeMimeHeader(sender)
  const match = decoded.match(/<([^>]+)>/)
  if (match) return match[1]
  return decoded
}

// check if the local part of an email looks like a machine-generated hash
function isMachineLocal(local: string): boolean {
  // long hex/uuid-like strings with dashes that mix digits and letters
  if (local.length <= 20) return false
  if (!/^[0-9a-f-]+$/i.test(local)) return false
  // must contain both digits and letters to look like a hash (not just repeated chars)
  return /[0-9]/.test(local) && /[a-f]/i.test(local)
}

// extract a human-readable display name from a "Name <email>" or raw email string
export function extractName(sender: string): string {
  // decode MIME encoded-words first
  const decoded = decodeMimeHeader(sender)
  const nameMatch = decoded.match(/^"?([^"<]+)"?\s*</)
  if (nameMatch) {
    const name = nameMatch[1].trim()
    // if the "name" part is actually a machine address, fall through to domain
    if (!name.includes('@') && !isMachineLocal(name)) return name
  }
  // fallback: use local part, or domain for machine-generated addresses
  const email = extractEmail(decoded)
  const [local, domain] = email.split('@')
  if (local && domain && isMachineLocal(local)) {
    // derive a readable label from the domain (e.g. "atlassian-bounces.atlassian.net" → "Atlassian")
    const parts = domain.split('.')
    const meaningful = parts.find((p) => !['com', 'net', 'org', 'io', 'co', 'jp', 'ai', 'mail', 'bounces', 'email', 'smtp', 'noreply', 'notifications'].includes(p) && !p.includes('bounce'))
    return meaningful ? meaningful.charAt(0).toUpperCase() + meaningful.slice(1) : domain
  }
  return local ?? sender
}
