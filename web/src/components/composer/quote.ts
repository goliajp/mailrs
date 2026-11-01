import type { ComposerStart } from './start'
import type { Translate } from '@/i18n'

import { formatFullDate } from '@/lib/format'
import { buildForwardHeaderHtml, escapeHtml } from '@/lib/html-utils'

export type Quote = { header?: string; headerHtml?: string; html?: string }

/** What the editor shows under the reply: the original and the line that introduces it. */
export function quoteOf(s: ComposerStart, t: Translate): Quote {
  if (s.kind === 'reply') {
    const line = t('compose.quoteHeader', {
      date: formatFullDate(s.reply.internalDate),
      sender: s.reply.sender,
    })
    return {
      header: `${line}\n\n`,
      headerHtml: `<p style="color:#888">${escapeHtml(line)}</p>`,
      html: s.reply.htmlBody ?? s.reply.textBody ?? undefined,
    }
  }
  if (s.kind !== 'inline') return {}
  const ctx = s.ctx
  if (s.mode === 'forward') {
    // the forwarded header stays in English: it is part of the message, not the interface
    return {
      header: `---------- Forwarded message ----------\nFrom: ${ctx.originalFrom}\nDate: ${ctx.originalDate}\nSubject: ${ctx.subject}\n\n`,
      headerHtml: buildForwardHeaderHtml(ctx.originalFrom, ctx.originalDate, ctx.subject),
      html: ctx.originalHtmlBody || ctx.originalBody || undefined,
    }
  }
  if (!ctx.originalFrom) return { html: ctx.originalBody || undefined }
  const line = t('compose.quoteHeader', { date: ctx.originalDate, sender: ctx.originalFrom })
  return {
    header: `${line}\n\n`,
    headerHtml: `<p style="color:#888">${escapeHtml(line)}</p>`,
    html: ctx.originalBody || undefined,
  }
}
