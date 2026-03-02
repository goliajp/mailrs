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
  // extract name or first char of email
  const match = sender.match(/^"?([^"<]+)"?\s*</)
  if (match) return match[1].trim()[0].toUpperCase()
  return (sender[0] ?? '?').toUpperCase()
}

export function extractEmail(sender: string): string {
  const match = sender.match(/<([^>]+)>/)
  if (match) return match[1]
  return sender
}

export function extractName(sender: string): string {
  const match = sender.match(/^"?([^"<]+)"?\s*</)
  if (match) return match[1].trim()
  return sender.split('@')[0]
}
