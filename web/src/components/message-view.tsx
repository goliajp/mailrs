import DOMPurify from 'dompurify'
import { useAtomValue, useSetAtom } from 'jotai'
import { useMemo, useState } from 'react'

import { splitEmail } from '@/lib/email-split'
import { formatFullDate, formatSize } from '@/lib/format'
import { composingAtom, selectedMessageDetailAtom } from '@/store/mail'

function sanitizeEmail(html: string): string {
  return DOMPurify.sanitize(html, {
    ALLOW_UNKNOWN_PROTOCOLS: false,
    ADD_ATTR: ['style', 'align', 'dir', 'bgcolor', 'color', 'face', 'size'],
    ADD_TAGS: ['style'],
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input'],
  })
}

function ViewSignature({
  signature,
  isHtml,
}: {
  signature: string
  isHtml: boolean
}) {
  return (
    <div className="mt-4 rounded-lg border border-zinc-200 bg-zinc-50 px-4 py-3 text-xs text-zinc-500 dark:border-zinc-700 dark:bg-zinc-800/50 dark:text-zinc-400">
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

function ViewQuoted({
  quoted,
  isHtml,
}: {
  quoted: string
  isHtml: boolean
}) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div className="mt-4">
      <button
        onClick={() => setExpanded(!expanded)}
        className="rounded px-2 py-1 text-xs text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-zinc-700 dark:hover:text-zinc-300"
      >
        {expanded ? '▲ Hide quoted text' : '··· Show quoted text'}
      </button>
      {expanded && (
        <div className="mt-2 border-l-2 border-zinc-300 pl-4 text-sm text-zinc-500 dark:border-zinc-600 dark:text-zinc-400">
          {isHtml ? (
            <div
              className="prose prose-sm max-w-none dark:prose-invert [&_*]:text-zinc-500 dark:[&_*]:text-zinc-400"
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

export function MessageView() {
  // all hooks MUST be called unconditionally before any return
  const msg = useAtomValue(selectedMessageDetailAtom)
  const setComposing = useSetAtom(composingAtom)

  const { parts, isHtml } = useMemo(
    () => splitEmail(msg?.text_body ?? null, msg?.html_body ?? null),
    [msg?.text_body, msg?.html_body]
  )

  const textBody = msg?.text_body ?? ''

  // conditional returns AFTER all hooks
  if (!msg) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-zinc-400">
        Select a message to read
      </div>
    )
  }

  return (
    <div className="flex flex-1 flex-col overflow-y-auto">
      <div className="border-b border-zinc-200 p-6 dark:border-zinc-800">
        <h2 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100">
          {msg.subject || '(no subject)'}
        </h2>
        <div className="mt-2 flex items-center gap-2 text-sm">
          <span className="font-medium text-zinc-700 dark:text-zinc-300">
            {msg.sender || '(unknown)'}
          </span>
        </div>
        {msg.recipients && (
          <div className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">
            To: {msg.recipients}
          </div>
        )}
        <div className="mt-1 text-xs text-zinc-400">
          {formatFullDate(msg.internal_date)}
        </div>
      </div>

      <div className="flex-1 p-6">
        {isHtml ? (
          <div
            className="prose prose-sm dark:prose-invert max-w-none"
            dangerouslySetInnerHTML={{ __html: sanitizeEmail(parts.body) }}
          />
        ) : (
          <pre className="font-sans text-sm leading-relaxed whitespace-pre-wrap break-words text-zinc-800 dark:text-zinc-200">
            {parts.body}
          </pre>
        )}

        {parts.signature && (
          <ViewSignature signature={parts.signature} isHtml={isHtml} />
        )}

        {parts.quoted && <ViewQuoted quoted={parts.quoted} isHtml={isHtml} />}
      </div>

      {msg.attachments.length > 0 && (
        <div className="border-t border-zinc-200 px-6 py-3 dark:border-zinc-800">
          <div className="text-xs font-medium text-zinc-500">
            Attachments ({msg.attachments.length})
          </div>
          <div className="mt-1 flex flex-wrap gap-2">
            {msg.attachments.map((att, i) => (
              <span
                key={i}
                className="rounded bg-zinc-100 px-2 py-1 text-xs dark:bg-zinc-800"
              >
                {att.filename} ({formatSize(att.size)})
              </span>
            ))}
          </div>
        </div>
      )}

      <div className="flex gap-2 border-t border-zinc-200 p-4 dark:border-zinc-800">
        <button
          onClick={() =>
            setComposing({
              to: msg.sender,
              cc: '',
              bcc: '',
              subject: `Re: ${msg.subject.replace(/^Re:\s*/i, '')}`,
              body: `\n\nOn ${formatFullDate(msg.internal_date)}, ${msg.sender} wrote:\n> ${textBody.split('\n').join('\n> ')}`,
            })
          }
          className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm transition-colors hover:bg-zinc-200 dark:bg-zinc-800 dark:hover:bg-zinc-700"
        >
          Reply
        </button>
        <button
          onClick={() =>
            setComposing({
              to: '',
              cc: '',
              bcc: '',
              subject: `Fwd: ${msg.subject.replace(/^Fwd:\s*/i, '')}`,
              body: `\n\n---------- Forwarded message ----------\nFrom: ${msg.sender}\nDate: ${formatFullDate(msg.internal_date)}\nSubject: ${msg.subject}\n\n${textBody}`,
            })
          }
          className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm transition-colors hover:bg-zinc-200 dark:bg-zinc-800 dark:hover:bg-zinc-700"
        >
          Forward
        </button>
      </div>
    </div>
  )
}
