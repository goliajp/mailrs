import type { ReactNode } from 'react'

// patterns that might refer to the current user
export function highlightMentions(
  text: string,
  myEmail: string,
  myName?: string
): ReactNode[] {
  if (!text || !myEmail) return [text]

  // build patterns: @email, @name, @firstname
  const patterns: string[] = []
  const emailLocal = myEmail.split('@')[0]
  if (emailLocal) patterns.push(emailLocal)
  if (myName) {
    patterns.push(myName)
    const firstName = myName.split(/\s+/)[0]
    if (firstName && firstName !== myName) patterns.push(firstName)
  }
  patterns.push(myEmail)

  // escape for regex
  const escaped = patterns.map((p) => p.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  // match @pattern or standalone pattern (case insensitive)
  const regex = new RegExp(
    `(@(?:${escaped.join('|')})|\\b(?:${escaped.join('|')})\\b)`,
    'gi'
  )

  const parts: ReactNode[] = []
  let lastIndex = 0
  let match: null | RegExpExecArray
  let key = 0

  while ((match = regex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push(text.slice(lastIndex, match.index))
    }
    parts.push(
      <mark className="bg-accent/10 text-accent px-0.5 font-medium" key={key++}>
        {match[0]}
      </mark>
    )
    lastIndex = regex.lastIndex
  }

  if (lastIndex < text.length) {
    parts.push(text.slice(lastIndex))
  }

  return parts.length > 0 ? parts : [text]
}
