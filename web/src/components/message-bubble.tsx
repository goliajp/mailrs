import DOMPurify from 'dompurify'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import Markdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'

import { splitEmail } from '@/lib/email-split'
import { formatSize } from '@/lib/format'
import type { AttachmentInfo } from '@/lib/types'

function looksLikeMarkdown(text: string): boolean {
  return /```|^#{1,6}\s|\*\*|__|\[.+\]\(.+\)/m.test(text)
}

const CJK_FONTS = "'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji'"

// inject CJK fallback fonts into all font-family declarations so kana
// renders correctly on non-Japanese locale systems
function injectCjkFonts(html: string): string {
  return html.replace(
    /font-family\s*:\s*([^;}"]+)/gi,
    (match, fonts: string) => {
      if (fonts.includes('Hiragino')) return match
      const trimmed = fonts.trimEnd()
      const endsWithSemiLike = trimmed.endsWith(',')
      const base = endsWithSemiLike ? trimmed.slice(0, -1) : trimmed
      return `font-family: ${base}, ${CJK_FONTS}`
    },
  )
}

function sanitizeEmail(html: string): string {
  // force all links to open in a new tab
  DOMPurify.addHook('afterSanitizeAttributes', (node) => {
    if (node.tagName === 'A') {
      node.setAttribute('target', '_blank')
      node.setAttribute('rel', 'noopener noreferrer')
    }
  })

  const clean = DOMPurify.sanitize(html, {
    ALLOW_UNKNOWN_PROTOCOLS: false,
    ADD_ATTR: ['style', 'align', 'dir', 'bgcolor', 'color', 'face', 'size', 'target', 'rel'],
    ADD_TAGS: ['style'],
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input'],
  })

  DOMPurify.removeHook('afterSanitizeAttributes')
  return injectCjkFonts(clean)
}

// render html email inside a sandboxed iframe for full css isolation
function HtmlFrame({ html }: { html: string }) {
  const ref = useRef<HTMLIFrameElement>(null)
  const [height, setHeight] = useState(200)

  const srcdoc = useMemo(() => {
    const sanitized = sanitizeEmail(html)
    return `<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<style>
  body { margin: 0; padding: 12px; font-family: -apple-system, BlinkMacSystemFont, 'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Segoe UI', Roboto, 'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji', sans-serif; font-size: 14px; line-height: 1.6; color: #1a1a1a; background: #fff; word-wrap: break-word; overflow-wrap: break-word; }
  img { max-width: 100%; height: auto; }
  table { max-width: 100% !important; }
  a { color: #2563eb; }
  pre { overflow-x: auto; }
  blockquote { border-left: 3px solid #d4d4d8; padding-left: 12px; margin: 8px 0; color: #71717a; }
</style>
</head><body>${sanitized}</body></html>`
  }, [html])

  const resize = useCallback(() => {
    const doc = ref.current?.contentDocument
    if (doc?.body) {
      const h = doc.body.scrollHeight
      if (h > 0) setHeight(h + 24)
    }
  }, [])

  useEffect(() => {
    const iframe = ref.current
    if (!iframe) return
    const onLoad = () => {
      resize()
      // observe content size changes (lazy-loaded images etc)
      const doc = iframe.contentDocument
      if (doc?.body) {
        const observer = new ResizeObserver(resize)
        observer.observe(doc.body)
        return () => observer.disconnect()
      }
    }
    iframe.addEventListener('load', onLoad)
    return () => iframe.removeEventListener('load', onLoad)
  }, [resize])

  return (
    <iframe
      ref={ref}
      srcDoc={srcdoc}
      sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
      style={{ width: '100%', height, border: 'none', display: 'block' }}
      title="email content"
    />
  )
}

function AttachmentItem({
  att,
  uid,
  index,
}: {
  att: AttachmentInfo
  uid: number
  index: number
}) {
  const isImage = att.content_type.startsWith('image/')
  const url = `/api/mail/messages/${uid}/attachments/${index}`

  return (
    <a
      href={url}
      target="_blank"
      rel="noopener noreferrer"
      className="flex items-center gap-2 rounded-md border border-zinc-200 px-3 py-2 text-sm transition-colors hover:bg-zinc-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
    >
      {isImage ? (
        <img
          src={url}
          alt={att.filename}
          loading="lazy"
          className="h-10 w-10 rounded object-cover"
        />
      ) : (
        <svg
          className="h-5 w-5 text-zinc-400"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z"
          />
        </svg>
      )}
      <div className="min-w-0 flex-1">
        <p className="truncate text-zinc-700 dark:text-zinc-300">
          {att.filename}
        </p>
        <p className="text-xs text-zinc-400">{formatSize(att.size)}</p>
      </div>
    </a>
  )
}

function TextContent({ body, isOwn }: { body: string; isOwn: boolean }) {
  if (looksLikeMarkdown(body)) {
    return (
      <div
        className={`prose prose-sm max-w-none ${
          isOwn
            ? '[&_*]:text-white [&_a]:text-blue-200 [&_code]:bg-blue-600'
            : 'dark:prose-invert'
        }`}
      >
        <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
          {body}
        </Markdown>
      </div>
    )
  }

  return (
    <pre
      className={`whitespace-pre-wrap break-words font-sans text-sm leading-relaxed ${
        isOwn ? 'text-white' : 'text-zinc-800 dark:text-zinc-200'
      }`}
    >
      {body}
    </pre>
  )
}

export function MessageBubble({
  uid,
  textBody,
  htmlBody,
  attachments,
  isOwn,
}: {
  uid: number
  textBody: string | null
  htmlBody: string | null
  attachments: AttachmentInfo[]
  isOwn: boolean
}) {
  const hasAttachments = attachments.length > 0
  const { parts, isHtml } = useMemo(
    () => splitEmail(textBody, htmlBody),
    [textBody, htmlBody],
  )

  return (
    <div>
      {isHtml ? (
        <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-zinc-700">
          <HtmlFrame html={parts.body} />
        </div>
      ) : (
        <div
          className={`rounded-2xl px-4 py-2.5 ${
            isOwn
              ? 'bg-blue-500 text-white'
              : 'bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
          }`}
        >
          <TextContent body={parts.body} isOwn={isOwn} />
        </div>
      )}

      {hasAttachments && (
        <div className="mt-2 flex flex-col gap-1">
          {attachments.map((att, i) => (
            <AttachmentItem key={i} att={att} uid={uid} index={i} />
          ))}
        </div>
      )}
    </div>
  )
}
