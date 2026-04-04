// decode RFC 2047 encoded-words in email headers (e.g. =?UTF-8?B?...?= or =?UTF-8?Q?...?=)
export function decodeMimeHeader(value: string): string {
  if (!value || !value.includes('=?')) return value ?? ''
  return value
    .replace(
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
            .replace(/=([0-9A-Fa-f]{2})/g, (_m, hex: string) =>
              String.fromCharCode(parseInt(hex, 16))
            )
          const bytes = new Uint8Array(decoded.length)
          for (let i = 0; i < decoded.length; i++) bytes[i] = decoded.charCodeAt(i)
          return new TextDecoder(charset).decode(bytes)
        } catch {
          return encoded
        }
      }
    )
    .replace(/\s+/g, ' ')
    .trim()
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
  return (match ? match[1] : decoded).toLowerCase()
}

// check if a string looks machine-generated (tracking IDs, bounce addresses, hashes)
export function isMachineGenerated(s: string): boolean {
  if (s.length <= 10) return false
  // known machine prefixes
  if (/^(bounce|msprvs|prvs)/i.test(s)) return true
  // VERP encoding embeds recipient with =
  if (s.length > 15 && s.includes('=')) return true
  const digits = s.replace(/[^0-9]/g, '').length
  // high digit ratio
  if (s.length > 12 && digits / s.length > 0.3) return true
  // long string without spaces that contains digits → not a human name
  // (human names either have spaces between words or are short)
  if (s.length > 20 && !s.includes(' ') && digits > 0) return true
  // low letter ratio (tracking IDs heavy on dots/dashes/equals)
  const letters = s.replace(/[^a-z]/gi, '').length
  if (s.length > 15 && letters / s.length < 0.5) return true
  return false
}

// extract the brand/registrable domain label
// e.g. "notify.cloudflare.com" → "cloudflare", "em8742.bsm.freee.work" → "freee"
const SECONDARY_TLDS = new Set(['ac', 'co', 'com', 'edu', 'gov', 'ne', 'net', 'or', 'org'])

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

function domainLabel(domain: string): string {
  const parts = domain.split('.')
  if (parts.length <= 1) return domain
  // the brand is usually the second-to-last part (just before the TLD)
  // for multi-part TLDs like .co.jp / .co.uk, go one level deeper
  const sld = parts[parts.length - 2]
  if (parts.length >= 3 && SECONDARY_TLDS.has(sld)) {
    return parts[parts.length - 3] || sld
  }
  return sld
}
