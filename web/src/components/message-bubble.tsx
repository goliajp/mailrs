import type { AttachmentInfo } from '@/lib/types'

import DOMPurify from 'dompurify'
import { File } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import Markdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'

import { MobileModal } from '@/components/mobile-modal'
import { RenderPreview } from '@/components/render-preview'
import { splitEmail } from '@/lib/email-split'
import { formatSize } from '@/lib/format'
import { getToken } from '@/store/auth'

function CodeBlock({ children, className, ...props }: React.HTMLAttributes<HTMLElement>) {
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
        <span className="text-fg-muted absolute top-2 right-10 text-xs opacity-100 transition-opacity md:text-[11px] md:opacity-0 md:group-hover:opacity-100">
          {lang}
        </span>
      )}
      <button
        aria-label={copied ? 'Copied to clipboard' : 'Copy code'}
        className="touch-target text-fg-muted hover:bg-bg-secondary hover:text-fg absolute top-2 right-2 rounded-md px-1.5 py-0.5 text-xs opacity-100 transition-opacity md:text-[11px] md:opacity-0 md:group-hover:opacity-100"
        onClick={copy}
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
      <code className={className} {...props}>
        {children}
      </code>
    </div>
  )
}

function looksLikeMarkdown(text: string): boolean {
  return /```|^#{1,6}\s|\*\*|__|\[.+\]\(.+\)/m.test(text)
}

const markdownComponents = {
  code: ({ children, className, ...props }: React.HTMLAttributes<HTMLElement>) => {
    const isBlock = className?.startsWith('language-') || String(children).includes('\n')
    if (isBlock) {
      return (
        <CodeBlock className={className} {...props}>
          {children}
        </CodeBlock>
      )
    }
    return (
      <code className={className} {...props}>
        {children}
      </code>
    )
  },
}

const CJK_FONTS =
  "'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji'"

// inject CJK fallback fonts into all font-family declarations so kana
// renders correctly on non-Japanese locale systems
function injectCjkFonts(html: string): string {
  return html.replace(/font-family\s*:\s*([^;}"]+)/gi, (match, fonts: string) => {
    if (fonts.includes('Hiragino')) return match
    const trimmed = fonts.trimEnd()
    const endsWithSemiLike = trimmed.endsWith(',')
    const base = endsWithSemiLike ? trimmed.slice(0, -1) : trimmed
    return `font-family: ${base}, ${CJK_FONTS}`
  })
}

// dedicated DOMPurify instance avoids global hook race conditions in concurrent renders
const emailPurifier = DOMPurify()
emailPurifier.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank')
    node.setAttribute('rel', 'noopener noreferrer')
  }
})

export function MessageBubble({
  attachments,
  htmlBody,
  isOwn,
  textBody,
  uid,
}: {
  attachments: AttachmentInfo[]
  htmlBody: null | string
  isOwn: boolean
  textBody: null | string
  uid: number
}) {
  const hasAttachments = attachments.length > 0
  const { isHtml, parts } = useMemo(() => splitEmail(textBody, htmlBody), [textBody, htmlBody])

  return (
    <div>
      {isHtml ? (
        <div>
          <div className="border-border bg-bg overflow-hidden border select-text">
            <HtmlFrame html={parts.body} />
          </div>
          <div className="px-2 py-1.5">
            <RenderPreview html={parts.body} />
          </div>
        </div>
      ) : (
        <div
          className={`px-4 py-2.5 select-text ${
            isOwn ? 'bg-accent text-white' : 'bg-surface text-fg'
          }`}
        >
          <TextContent body={parts.body} isOwn={isOwn} />
        </div>
      )}

      {hasAttachments && (
        <div className="mt-2 flex flex-col gap-1">
          {attachments.map((att, i) => (
            <AttachmentItem att={att} index={i} key={i} uid={uid} />
          ))}
        </div>
      )}
    </div>
  )
}

function AttachmentItem({ att, index, uid }: { att: AttachmentInfo; index: number; uid: number }) {
  const isImage = att.content_type.startsWith('image/')
  const isPdf = att.content_type === 'application/pdf'
  const token = getToken() ?? ''
  const url = `/api/mail/messages/${uid}/attachments/${index}?token=${encodeURIComponent(token)}`
  const [lightbox, setLightbox] = useState(false)
  const [showContent, setShowContent] = useState(false)
  const [extractedText, setExtractedText] = useState<null | string>(null)
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
      if (!res.ok) {
        setExtractedText('')
        setShowContent(false)
        return
      }
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
    <div className="border-border border">
      <div className="flex items-center gap-2 px-3 py-2 text-sm">
        {isImage ? (
          <img
            alt={att.filename}
            className="h-10 w-10 cursor-pointer rounded-md object-cover"
            loading="lazy"
            onClick={() => setLightbox(true)}
            src={url}
          />
        ) : (
          <a href={url} rel="noopener noreferrer" target="_blank">
            <File className="text-fg-muted h-5 w-5" />
          </a>
        )}
        <div className="min-w-0 flex-1">
          <a
            className="text-fg-secondary block truncate hover:underline"
            href={url}
            rel="noopener noreferrer"
            target="_blank"
          >
            {att.filename}
          </a>
          <p className="text-fg-muted text-xs">{formatSize(att.size)}</p>
        </div>
        {isExtractable(att.content_type) && (
          <button
            className="text-fg-muted hover:bg-bg-secondary shrink-0 rounded-md px-2 py-0.5 text-xs transition-colors"
            disabled={loading}
            onClick={fetchContent}
            title="Show extracted text"
            type="button"
          >
            {loading ? '...' : showContent ? 'Hide text' : 'OCR'}
          </button>
        )}
      </div>

      {/* extracted text panel */}
      {showContent && extractedText && (
        <div className="border-border border-t px-3 py-2">
          <pre className="text-fg-secondary max-h-48 overflow-auto text-xs whitespace-pre-wrap">
            {extractedText}
          </pre>
        </div>
      )}

      {/* image lightbox */}
      {lightbox && isImage && (
        <MobileModal className="bg-black/60" onClose={() => setLightbox(false)} open>
          <div onClick={(e) => e.stopPropagation()}>
            <img
              alt={att.filename}
              className="max-h-[90vh] max-w-[90vw] rounded-md object-contain"
              src={url}
            />
            <button
              className="absolute top-4 right-4 rounded-md bg-black/50 px-3 py-1 text-white transition-colors hover:bg-black/70"
              onClick={() => setLightbox(false)}
              type="button"
            >
              &times;
            </button>
          </div>
        </MobileModal>
      )}

      {/* PDF inline preview — sandbox restricts script execution in PDF viewers */}
      {isPdf && showContent && (
        <div className="border-border border-t">
          <iframe
            className="h-96 w-full"
            sandbox="allow-same-origin"
            src={url}
            title={att.filename}
          />
        </div>
      )}
    </div>
  )
}

// render html email inside a sandboxed iframe for full css isolation.
// CSS containment + isolation on the wrapper guarantee the email's layout
// can never bleed into the surrounding app, even if a clever payload tries.
// wide content is fitted via transform: scale() — `zoom` is non-standard and
// in some Blink layouts can disturb sibling/parent metrics
function HtmlFrame({ html }: { html: string }) {
  const ref = useRef<HTMLIFrameElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState(200)
  const [scale, setScale] = useState(1)
  const [containerWidth, setContainerWidth] = useState(0)

  const srcdoc = useMemo(() => {
    const sanitized = sanitizeEmail(html)
    return `<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="referrer" content="no-referrer">
<style>
  body { margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, 'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Segoe UI', Roboto, 'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji', sans-serif; font-size: 14px; line-height: 1.6; color: #1a1a1a; background: #fff; word-wrap: break-word; overflow-wrap: break-word; }
  .mail-wrap { max-width: 680px; margin: 0 auto; padding: 12px; box-sizing: border-box; }
  img { max-width: 100%; height: auto; }
  a { color: #2563eb; }
  pre { overflow-x: auto; }
  blockquote { border-left: 3px solid #d4d4d8; padding-left: 12px; margin: 8px 0; color: #71717a; }
</style>
</head><body><div class="mail-wrap">${sanitized}</div></body></html>`
  }, [html])

  const measure = useCallback(() => {
    const iframe = ref.current
    const container = containerRef.current
    if (!iframe || !container) return
    const doc = iframe.contentDocument
    if (!doc?.body) return

    const contentW = doc.body.scrollWidth
    const containerW = container.clientWidth
    const contentH = doc.body.scrollHeight

    const s = contentW > containerW && containerW > 0 ? containerW / contentW : 1
    setScale(s)
    setContainerWidth(containerW)
    setHeight(contentH * s + 24)
  }, [])

  useEffect(() => {
    const iframe = ref.current
    if (!iframe) return
    const onLoad = () => {
      measure()
      const doc = iframe.contentDocument
      if (doc?.body) {
        const obs = new ResizeObserver(measure)
        obs.observe(doc.body)
        return () => obs.disconnect()
      }
    }
    iframe.addEventListener('load', onLoad)
    return () => iframe.removeEventListener('load', onLoad)
  }, [measure])

  // re-measure on container resize (orientation change)
  useEffect(() => {
    const c = containerRef.current
    if (!c) return
    const obs = new ResizeObserver(measure)
    obs.observe(c)
    return () => obs.disconnect()
  }, [measure])

  // when scaling down, give the iframe its natural width so the content
  // doesn't reflow under the smaller container; the transform shrinks it
  // back into view, and the wrapper's contain locks the box so nothing
  // bleeds out
  const iframeWidth = scale < 1 && containerWidth > 0 ? `${containerWidth / scale}px` : '100%'
  const iframeHeight = scale < 1 ? `${height / scale}px` : `${height}px`

  return (
    <div
      className="relative isolate overflow-hidden [contain:layout_style_paint]"
      ref={containerRef}
      style={{ height: `${height}px` }}
    >
      <iframe
        className="block origin-top-left border-none"
        ref={ref}
        sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
        srcDoc={srcdoc}
        style={{
          height: iframeHeight,
          transform: scale < 1 ? `scale(${scale})` : undefined,
          width: iframeWidth,
        }}
        title="email content"
      />
    </div>
  )
}

// check if content type supports OCR/text extraction
function isExtractable(contentType: string): boolean {
  return contentType.startsWith('image/') || contentType === 'application/pdf'
}

// rewrite external image URLs to go through our proxy (bypasses CSP img-src 'self')
function proxyExternalUrls(html: string): string {
  const token = getToken()
  const tokenParam = token ? `&token=${encodeURIComponent(token)}` : ''
  // proxy images
  let result = html.replace(
    /(<img\b[^>]*\bsrc\s*=\s*["'])(https?:\/\/[^"']+)(["'])/gi,
    (_match, before, url, after) =>
      `${before}/api/proxy/image?url=${encodeURIComponent(url)}${tokenParam}${after}`
  )
  // proxy links for safe browsing
  result = result.replace(
    /(<a\b[^>]*\bhref\s*=\s*["'])(https?:\/\/[^"']+)(["'])/gi,
    (_match, before, url, after) =>
      `${before}/api/proxy/link?url=${encodeURIComponent(url)}${tokenParam}${after}`
  )
  return result
}

function sanitizeEmail(html: string): string {
  const clean = emailPurifier.sanitize(html, {
    ADD_ATTR: ['style', 'align', 'dir', 'bgcolor', 'color', 'face', 'size', 'target', 'rel'],
    ADD_TAGS: ['style'],
    ALLOW_UNKNOWN_PROTOCOLS: false,
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input'],
  })
  return proxyExternalUrls(injectCjkFonts(clean))
}

function TextContent({ body, isOwn }: { body: string; isOwn: boolean }) {
  if (looksLikeMarkdown(body)) {
    return (
      <div
        className={`prose prose-sm max-w-none ${
          isOwn ? '[&_*]:text-white [&_a]:text-white/70 [&_code]:bg-white/20' : 'prose-fg'
        }`}
      >
        <Markdown
          components={markdownComponents}
          rehypePlugins={[rehypeHighlight]}
          remarkPlugins={[remarkGfm]}
        >
          {body}
        </Markdown>
      </div>
    )
  }

  return (
    <pre
      className={`font-sans text-sm leading-relaxed break-words whitespace-pre-wrap ${
        isOwn ? 'text-white' : 'text-fg'
      }`}
    >
      {body}
    </pre>
  )
}
