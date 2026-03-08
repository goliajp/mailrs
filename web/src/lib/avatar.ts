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

// check if a string looks machine-generated (tracking IDs, bounce addresses, hashes)
export function isMachineGenerated(s: string): boolean {
  // short strings are likely human
  if (s.length <= 10) return false
  // known machine prefixes (VERP bounce, prvs anti-spam)
  if (/^(bounce|msprvs|prvs)\b/i.test(s)) return true
  const digits = s.replace(/[^0-9]/g, '').length
  const letters = s.replace(/[^a-z]/gi, '').length
  // high digit ratio in a reasonably long string → machine
  if (s.length > 12 && digits / s.length > 0.3) return true
  // low letter ratio in a long string → machine (tracking IDs with dots/dashes/equals)
  if (s.length > 15 && letters / s.length < 0.5) return true
  return false
}

// extract the registrable domain label (e.g. "notify.cloudflare.com" → "cloudflare")
const TLDS = new Set(['com', 'net', 'org', 'io', 'co', 'jp', 'ai', 'uk', 'au', 'de', 'fr', 'cn', 'kr', 'in', 'br', 'ru', 'es', 'it', 'nl', 'se', 'no', 'fi', 'dk', 'pt', 'pl', 'cz', 'at', 'ch', 'be', 'ie', 'nz', 'sg', 'hk', 'tw', 'th', 'my', 'ph', 'id', 'vn'])

function domainLabel(domain: string): string {
  const parts = domain.split('.')
  // walk from the end to skip TLD parts, then return the first meaningful part
  let i = parts.length - 1
  while (i > 0 && TLDS.has(parts[i])) i--
  return parts[i] || domain
}

// extract a human-readable display name from a "Name <email>" or raw email string
export function extractName(sender: string): string {
  // decode MIME encoded-words first
  const decoded = decodeMimeHeader(sender)
  const nameMatch = decoded.match(/^"?([^"<]+)"?\s*</)
  if (nameMatch) {
    const name = nameMatch[1].trim()
    // if the "name" part is actually a machine address, fall through to domain
    if (!name.includes('@') && !isMachineGenerated(name)) return name
  }
  // fallback: use local part, or domain for machine-generated addresses
  const email = extractEmail(decoded)
  const [local, domain] = email.split('@')
  if (local && domain && isMachineGenerated(local)) {
    // derive a readable label from the registrable domain name
    const label = domainLabel(domain)
    return label.charAt(0).toUpperCase() + label.slice(1)
  }
  return local ?? sender
}
