import DOMPurify from 'dompurify'
import { File } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import Markdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'

import { splitEmail } from '@/lib/email-split'
import { formatSize } from '@/lib/format'
import type { AttachmentInfo } from '@/lib/types'
import { getToken } from '@/store/auth'

function looksLikeMarkdown(text: string): boolean {
  return /```|^#{1,6}\s|\*\*|__|\[.+\]\(.+\)/m.test(text)
}

function CodeBlock({ className, children, ...props }: React.HTMLAttributes<HTMLElement>) {
  const [copied, setCopied] = useState(false)
  const code = String(children).replace(/\n$/, '')
  const lang = className?.replace('language-', '') ?? ''

  const copy = () => {
    navigator.clipboard.writeText(code)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className="group relative overflow-hidden">
      {lang && (
        <span className="absolute right-10 top-2 text-[11px] text-[var(--color-text-tertiary)] opacity-0 transition-opacity group-hover:opacity-100">
          {lang}
        </span>
      )}
      <button
        onClick={copy}
        aria-label={copied ? 'Copied to clipboard' : 'Copy code'}
        className="absolute right-2 top-2 rounded-md px-1.5 py-0.5 text-[11px] text-[var(--color-text-tertiary)] opacity-0 transition-opacity hover:bg-[var(--color-active)] hover:text-[var(--color-text-primary)] group-hover:opacity-100"
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
      <code className={className} {...props}>
        {children}
      </code>
    </div>
  )
}

const markdownComponents = {
  code: ({ className, children, ...props }: React.HTMLAttributes<HTMLElement>) => {
    const isBlock = className?.startsWith('language-') || String(children).includes('\n')
    if (isBlock) {
      return <CodeBlock className={className} {...props}>{children}</CodeBlock>
    }
    return <code className={className} {...props}>{children}</code>
  },
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

// dedicated DOMPurify instance avoids global hook race conditions in concurrent renders
const emailPurifier = DOMPurify()
emailPurifier.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank')
    node.setAttribute('rel', 'noopener noreferrer')
  }
})

function sanitizeEmail(html: string): string {
  const clean = emailPurifier.sanitize(html, {
    ALLOW_UNKNOWN_PROTOCOLS: false,
    ADD_ATTR: ['style', 'align', 'dir', 'bgcolor', 'color', 'face', 'size', 'target', 'rel'],
    ADD_TAGS: ['style'],
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input'],
  })
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
      className="block w-full border-none"
      style={{ height }}
      title="email content"
    />
  )
}

// check if content type supports OCR/text extraction
function isExtractable(contentType: string): boolean {
  return (
    contentType.startsWith('image/') ||
    contentType === 'application/pdf'
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
  const isPdf = att.content_type === 'application/pdf'
  const token = getToken() ?? ''
  const url = `/api/mail/messages/${uid}/attachments/${index}?token=${encodeURIComponent(token)}`
  const [lightbox, setLightbox] = useState(false)
  const [showContent, setShowContent] = useState(false)
  const [extractedText, setExtractedText] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const fetchContent = useCallback(async () => {
    if (extractedText !== null) {
      setShowContent((v) => !v)
      return
    }
    setLoading(true)
    try {
      const t = getToken()
      const headers: Record<string, string> = {}
      if (t) headers['Authorization'] = `Bearer ${t}`
      const res = await fetch(`/api/mail/messages/${uid}/attachments/${index}/content`, { headers })
      if (!res.ok) { setExtractedText(''); setShowContent(false); return }
      const data = await res.json()
      if (data.success && data.extracted_text) {
        setExtractedText(data.extracted_text)
        setShowContent(true)
      } else {
        setExtractedText('')
        setShowContent(false)
      }
    } catch {
      setExtractedText('')
    } finally {
      setLoading(false)
    }
  }, [uid, index, extractedText])

  return (
    <div className="border border-[var(--color-border-default)]">
      <div className="flex items-center gap-2 px-3 py-2 text-sm">
        {isImage ? (
          <img
            src={url}
            alt={att.filename}
            loading="lazy"
            className="h-10 w-10 cursor-pointer rounded-md object-cover"
            onClick={() => setLightbox(true)}
          />
        ) : (
          <a href={url} target="_blank" rel="noopener noreferrer">
            <File className="h-5 w-5 text-[var(--color-text-tertiary)]" />
          </a>
        )}
        <div className="min-w-0 flex-1">
          <a
            href={url}
            target="_blank"
            rel="noopener noreferrer"
            className="block truncate text-[var(--color-text-secondary)] hover:underline"
          >
            {att.filename}
          </a>
          <p className="text-xs text-[var(--color-text-tertiary)]">{formatSize(att.size)}</p>
        </div>
        {isExtractable(att.content_type) && (
          <button
            type="button"
            onClick={fetchContent}
            disabled={loading}
            className="shrink-0 rounded-md px-2 py-0.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
            title="Show extracted text"
          >
            {loading ? '...' : showContent ? 'Hide text' : 'OCR'}
          </button>
        )}
      </div>

      {/* extracted text panel */}
      {showContent && extractedText && (
        <div className="border-t border-[var(--color-border-default)] px-3 py-2">
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap text-xs text-[var(--color-text-secondary)]">
            {extractedText}
          </pre>
        </div>
      )}

      {/* image lightbox */}
      {lightbox && isImage && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
          onClick={() => setLightbox(false)}
        >
          <img
            src={url}
            alt={att.filename}
            className="max-h-[90vh] max-w-[90vw] rounded-md object-contain"
            onClick={(e) => e.stopPropagation()}
          />
          <button
            type="button"
            className="absolute right-4 top-4 rounded-md bg-black/50 px-3 py-1 text-white transition-colors hover:bg-black/70"
            onClick={() => setLightbox(false)}
          >
            &times;
          </button>
        </div>
      )}

      {/* PDF inline preview — sandbox restricts script execution in PDF viewers */}
      {isPdf && showContent && (
        <div className="border-t border-[var(--color-border-default)]">
          <iframe
            src={url}
            className="h-96 w-full"
            title={att.filename}
            sandbox="allow-same-origin"
          />
        </div>
      )}
    </div>
  )
}

function TextContent({ body, isOwn }: { body: string; isOwn: boolean }) {
  if (looksLikeMarkdown(body)) {
    return (
      <div
        className={`prose prose-sm max-w-none ${
          isOwn
            ? '[&_*]:text-white [&_a]:text-white/70 [&_code]:bg-white/20'
            : 'prose-[var(--color-text-primary)]'
        }`}
      >
        <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]} components={markdownComponents}>
          {body}
        </Markdown>
      </div>
    )
  }

  return (
    <pre
      className={`whitespace-pre-wrap break-words font-sans text-sm leading-relaxed ${
        isOwn ? 'text-white' : 'text-[var(--color-text-primary)]'
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
        <div className="select-text overflow-hidden border border-[var(--color-border-default)] bg-[var(--color-bg-base)]">
          <HtmlFrame html={parts.body} />
        </div>
      ) : (
        <div
          className={`select-text px-4 py-2.5 ${
            isOwn
              ? 'bg-[var(--color-brand-primary)] text-white'
              : 'bg-[var(--color-bg-raised)] text-[var(--color-text-primary)]'
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
