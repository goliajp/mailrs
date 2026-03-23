export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)}KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
}

export function buildForwardHeader(from: string, date: string, subject: string): string {
  return `---------- Forwarded message ----------\nFrom: ${from}\nDate: ${date}\nSubject: ${subject}\n`
}

export function buildForwardHeaderHtml(from: string, date: string, subject: string): string {
  return `<p style="color:#888">---------- Forwarded message ----------<br>From: ${escapeHtml(from)}<br>Date: ${escapeHtml(date)}<br>Subject: ${escapeHtml(subject)}</p>`
}
