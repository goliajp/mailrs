import type { ThreadMessage } from '@/lib/types'

import { toast } from '@goliapkg/gds'
import DOMPurify from 'dompurify'

import { formatFullDate } from '@/lib/format'
import { getToken } from '@/store/auth'

export async function downloadEml(uid: number, subject: string) {
  try {
    const token = getToken()
    const headers: Record<string, string> = {}
    if (token) headers['Authorization'] = `Bearer ${token}`
    const res = await fetch(`/api/mail/messages/${uid}/raw`, { headers })
    if (!res.ok) {
      toast.error('Download failed')
      return
    }
    const blob = await res.blob()
    const safeName = subject.replace(/[^a-zA-Z0-9\u4e00-\u9fff\u3040-\u30ff _-]/g, '_').trim()
    const url = URL.createObjectURL(blob)
    try {
      const a = document.createElement('a')
      a.href = url
      a.download = safeName ? `${safeName}.eml` : `message-${uid}.eml`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
    } finally {
      setTimeout(() => URL.revokeObjectURL(url), 1000)
    }
  } catch {
    toast.error('Download failed')
  }
}

// open a print window for one message. the whole document is built as a
// string because the print window is a separate document with no styles
// of ours in it.
export function printMessage(msg: ThreadMessage) {
  const w = window.open('', '_blank')
  if (!w) return
  const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
  const body = msg.html_body
    ? DOMPurify.sanitize(msg.html_body)
    : `<pre style="white-space:pre-wrap;word-break:break-word;font-family:sans-serif;font-size:14px;line-height:1.6">${esc(msg.clean_text || msg.text_body || '')}</pre>`
  w.document.write(
    `<!DOCTYPE html><html><head><meta charset="utf-8"><title>${esc(msg.subject || '')}</title><style>body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;padding:2rem;max-width:800px;margin:0 auto}table{border-collapse:collapse;width:100%;margin-bottom:1.5rem}td{padding:4px 8px;font-size:14px}td:first-child{font-weight:600;white-space:nowrap;color:#555;width:80px}hr{border:none;border-top:1px solid #ddd;margin:1rem 0}img{max-width:100%}@media print{body{padding:0}}</style></head><body><table><tr><td>From</td><td>${esc(msg.sender)}</td></tr><tr><td>To</td><td>${esc(msg.recipients)}</td></tr>${msg.cc ? `<tr><td>Cc</td><td>${esc(msg.cc)}</td></tr>` : ''}<tr><td>Date</td><td>${esc(formatFullDate(msg.internal_date))}</td></tr><tr><td>Subject</td><td>${esc(msg.subject || '')}</td></tr></table><hr><div>${body}</div></body></html>`
  )
  w.document.close()
  w.onload = () => w.print()
}
