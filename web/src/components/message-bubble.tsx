import DOMPurify from 'dompurify'
import { useMemo, useState } from 'react'
import Markdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'

import { splitEmail } from '@/lib/email-split'
import { formatSize } from '@/lib/format'
import type { AttachmentInfo } from '@/lib/types'

// only render as markdown if the text contains markdown-specific syntax
// plain email replies should not be interpreted as markdown
function looksLikeMarkdown(text: string): boolean {
  return /```|^#{1,6}\s|\*\*|__|\[.+\]\(.+\)/m.test(text)
}

// sanitize HTML email body while preserving safe inline styles
function sanitizeEmail(html: string): string {
  return DOMPurify.sanitize(html, {
    ALLOW_UNKNOWN_PROTOCOLS: false,
    ADD_ATTR: ['style', 'align', 'dir', 'bgcolor', 'color', 'face', 'size'],
    ADD_TAGS: ['style'],
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input'],
  })
}

function SignatureCard({
  signature,
  isHtml,
}: {
  signature: string
  isHtml: boolean
}) {
  return (
    <div className="mt-1.5 rounded-lg bg-zinc-50 px-3 py-2 text-xs text-zinc-500 dark:bg-zinc-800/50 dark:text-zinc-400">
      {isHtml ? (
        <div dangerouslySetInnerHTML={{ __html: sanitizeEmail(signature) }} />
      ) : (
        <pre className="whitespace-pre-wrap break-words font-sans">
          {signature}
        </pre>
      )}
    </div>
  )
}

function QuotedText({
  quoted,
  isHtml,
}: {
  quoted: string
  isHtml: boolean
}) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div className="mt-1.5">
      <button
        onClick={() => setExpanded(!expanded)}
        className="rounded px-2 py-0.5 text-xs text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-700 dark:hover:text-zinc-300"
      >
        {expanded ? '▲' : '···'}
      </button>
      {expanded && (
        <div className="mt-1 border-l-2 border-zinc-300 pl-3 text-sm text-zinc-500 dark:border-zinc-600 dark:text-zinc-400">
          {isHtml ? (
            <div
              className="prose prose-sm max-w-none prose-p:my-1 dark:prose-invert [&_*]:text-zinc-500 dark:[&_*]:text-zinc-400"
              dangerouslySetInnerHTML={{ __html: sanitizeEmail(quoted) }}
            />
          ) : (
            <pre className="whitespace-pre-wrap break-words font-sans">
              {quoted}
            </pre>
          )}
        </div>
      )}
    </div>
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

function BodyContent({
  body,
  isHtml,
  isOwn,
}: {
  body: string
  isHtml: boolean
  isOwn: boolean
}) {
  if (isHtml) {
    return (
      <div
        className={`prose prose-sm max-w-none ${
          isOwn
            ? '[&_*]:text-white [&_a]:text-blue-200'
            : 'dark:prose-invert'
        }`}
        dangerouslySetInnerHTML={{
          __html: sanitizeEmail(body),
        }}
      />
    )
  }

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
    [textBody, htmlBody]
  )

  return (
    <div>
      <div
        className={`rounded-2xl px-4 py-2.5 ${
          isOwn
            ? 'bg-blue-500 text-white'
            : 'bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
        }`}
      >
        <BodyContent body={parts.body} isHtml={isHtml} isOwn={isOwn} />
      </div>

      {parts.signature && (
        <SignatureCard signature={parts.signature} isHtml={isHtml} />
      )}

      {parts.quoted && <QuotedText quoted={parts.quoted} isHtml={isHtml} />}

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
