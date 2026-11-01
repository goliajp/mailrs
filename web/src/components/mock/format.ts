/** `received_at` is milliseconds, shown in `locale`. */
export function formatReceived(ms: number, locale: string): string {
  return new Date(ms).toLocaleString(locale, {
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    month: 'short',
    second: '2-digit',
  })
}

/** Comma- or newline-separated addresses, blanks dropped. */
export function splitRecipients(raw: string): string[] {
  return raw
    .split(/[,\n]/)
    .map((a) => a.trim())
    .filter((a) => a !== '')
}

export function subjectLabel(subject: string, noSubject: string): string {
  if (subject === '') return noSubject
  return subject
}
