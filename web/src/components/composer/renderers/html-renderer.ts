import type { AnyBlock } from '../types'

import { marked } from 'marked'

import { escapeHtml } from '@/lib/html-utils'

export function renderBlockHtml(block: AnyBlock): string {
  switch (block.type) {
    case 'attachment': {
      const d = block.data as { mimeType: string; name: string; size: number }
      const sizeStr =
        d.size < 1024 * 1024
          ? `${(d.size / 1024).toFixed(0)}KB`
          : `${(d.size / (1024 * 1024)).toFixed(1)}MB`
      return `<div style="margin:8px 0;padding:8px 12px;border:1px solid #ddd;border-radius:8px;display:inline-block">
        <span style="font-size:13px">📎 ${escapeHtml(d.name)}</span>
        <span style="font-size:11px;color:#888;margin-left:8px">${sizeStr}</span>
      </div>`
    }

    case 'code': {
      const d = block.data as { code: string; language: string }
      const lang = d.language ? ` (${escapeHtml(d.language)})` : ''
      return `<div style="margin:12px 0">
        <pre style="background:#1e1e2e;color:#cdd6f4;border-radius:8px;padding:12px 16px;font-family:'SF Mono',Monaco,Consolas,monospace;font-size:13px;line-height:1.5;overflow-x:auto;white-space:pre-wrap"><code>${escapeHtml(d.code)}</code></pre>
        ${lang ? `<p style="font-size:11px;color:#888;margin:4px 0 0">${lang}</p>` : ''}
      </div>`
    }

    case 'divider':
      return '<hr style="border:none;border-top:1px solid #ddd;margin:16px 0">'

    case 'quote': {
      const d = block.data as { headerHtml: string; html: string }
      return `<div style="margin-top:16px">${d.headerHtml}<blockquote style="margin:8px 0;padding-left:12px;border-left:3px solid #ccc;color:#888">${d.html}</blockquote></div>`
    }

    case 'signature': {
      const d = block.data as { html: string }
      return `<div style="margin-top:16px;color:#888"><p style="margin:0">-- </p>${d.html}</div>`
    }

    case 'task': {
      const d = block.data as {
        items: ReadonlyArray<{ checked: boolean; text: string }>
      }
      const items = d.items
        .map((item) => {
          const check = item.checked ? '☑' : '☐'
          const style = item.checked
            ? 'text-decoration:line-through;color:#888'
            : ''
          return `<li style="list-style:none;padding:2px 0;${style}">${check} ${escapeHtml(item.text)}</li>`
        })
        .join('')
      return `<ul style="padding-left:0;margin:8px 0">${items}</ul>`
    }

    case 'text': {
      const d = block.data as { content: string; format: string; html: string }
      if (d.format === 'markdown') {
        return marked.parse(d.content, {
          async: false,
          breaks: true,
          gfm: true,
        }) as string
      }
      return d.html
    }

    default:
      return ''
  }
}

export function renderBlockText(block: AnyBlock): string {
  switch (block.type) {
    case 'attachment': {
      const d = block.data as { name: string }
      return `[Attachment: ${d.name}]`
    }

    case 'code': {
      const d = block.data as { code: string; language: string }
      return `\`\`\`${d.language}\n${d.code}\n\`\`\``
    }

    case 'divider':
      return '\n---\n'

    case 'quote': {
      const d = block.data as { headerText: string; html: string }
      // extract text from html
      const textContent = d.html.replace(/<[^>]*>/g, '').trim()
      const quoted = textContent
        .split('\n')
        .map((l) => `> ${l}`)
        .join('\n')
      return `\n${d.headerText}\n${quoted}`
    }

    case 'signature': {
      const d = block.data as { text: string }
      return `\n-- \n${d.text}`
    }

    case 'task': {
      const d = block.data as {
        items: ReadonlyArray<{ checked: boolean; text: string }>
      }
      return d.items
        .map((item) => `${item.checked ? '[x]' : '[ ]'} ${item.text}`)
        .join('\n')
    }

    case 'text': {
      const d = block.data as { content: string }
      return d.content
    }

    default:
      return ''
  }
}
