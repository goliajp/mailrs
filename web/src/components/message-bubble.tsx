import type { AttachmentInfo } from '@/lib/types'

import { File } from 'lucide-react'
import { lazy, memo, Suspense, useCallback, useDeferredValue, useMemo, useState } from 'react'

import { HtmlFrame } from '@/components/html-frame'
import { MobileModal } from '@/components/mobile-modal'
import { RenderPreview } from '@/components/render-preview'
import { splitEmail } from '@/lib/email-split'
import { formatSize } from '@/lib/format'
import { getToken } from '@/store/auth'

// react-markdown + remark-gfm + rehype-highlight together drag in roughly
// 150-200 kB of JS once highlight.js's language pack is in the graph. plain-
// text emails (the overwhelming majority) never need this code path. lazy()
// keeps it out of the eager chat chunk; <Suspense> falls back to the plain
// <pre> rendering while the chunk fetches.
const MarkdownViewer = lazy(() => import('@/components/markdown-viewer'))

function looksLikeMarkdown(text: string): boolean {
  return /```|^#{1,6}\s|\*\*|__|\[.+\]\(.+\)/m.test(text)
}

export const MessageBubble = memo(function MessageBubble({
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
  // Defer the body inputs through `useDeferredValue` so that clicking a new
  // conversation commits the shell (header, attachments row) at user-input
  // priority and the heavy `splitEmail` work — `new DOMParser()` over a
  // newsletter-sized HTML body costs 50-200ms — runs at transition priority
  // in the next frame instead of freezing the click commit.
  const deferredTextBody = useDeferredValue(textBody)
  const deferredHtmlBody = useDeferredValue(htmlBody)
  const { isHtml, parts } = useMemo(
    () => splitEmail(deferredTextBody, deferredHtmlBody),
    [deferredTextBody, deferredHtmlBody]
  )

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
})

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

// check if content type supports OCR/text extraction
function isExtractable(contentType: string): boolean {
  return contentType.startsWith('image/') || contentType === 'application/pdf'
}

function PlainPre({ body, isOwn }: { body: string; isOwn: boolean }) {
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

function TextContent({ body, isOwn }: { body: string; isOwn: boolean }) {
  if (looksLikeMarkdown(body)) {
    return (
      <div
        className={`prose prose-sm max-w-none ${
          isOwn ? '[&_*]:text-white [&_a]:text-white/70 [&_code]:bg-white/20' : 'prose-fg'
        }`}
      >
        <Suspense fallback={<PlainPre body={body} isOwn={isOwn} />}>
          <MarkdownViewer body={body} />
        </Suspense>
      </div>
    )
  }

  return <PlainPre body={body} isOwn={isOwn} />
}
