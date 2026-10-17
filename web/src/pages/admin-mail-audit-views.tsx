import type { AttachmentInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { ChevronLeft, Download } from 'lucide-react'

import { HtmlFrame } from '@/components/html-frame'
import { adminKeys } from '@/lib/query-keys'
import { getToken } from '@/store/auth'
import { adminListGet } from '@/wire/endpoints/admin'

/** One row of `GET /api/admin/audit/conversations`. */
export type AuditConversation = {
  last_date: number
  message_count: number
  /** comma-separated, as the core stores it */
  participants: string
  snippet: string
  subject: string
  thread_id: string
}

/** One row of `GET /api/admin/audit/conversations/{id}/messages`. */
export type AuditMessage = {
  attachments: AttachmentInfo[]
  html_body: null | string
  internal_date: number
  recipients: string
  risk_score: number
  sender: string
  subject: string
  text_body: null | string
  uid: number
}

const enc = encodeURIComponent
const NO_CONVERSATIONS: AuditConversation[] = []
const NO_MESSAGES: AuditMessage[] = []

export function AuditConversations({
  account,
  onBack,
  onOpen,
}: {
  account: string
  onBack: () => void
  onOpen: (threadId: string) => void
}) {
  const { data, isFetching } = useQuery({
    queryKey: adminKeys.mailAuditConversations(account),
    queryFn: ({ signal }) =>
      adminListGet<AuditConversation>(
        `/admin/audit/conversations?user=${enc(account)}&limit=50`,
        signal
      ),
  })
  const conversations = data ?? NO_CONVERSATIONS

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <AuditHeader backLabel="Back to accounts" onBack={onBack} title={account} />
      <div className="flex-1 overflow-y-auto">
        {isFetching && conversations.length === 0 && <Note text="Loading…" />}
        {conversations.map((c) => (
          <button
            className="border-border hover:bg-bg-secondary flex w-full items-start gap-3 border-b px-6 py-3 text-left transition-colors"
            key={c.thread_id}
            onClick={() => onOpen(c.thread_id)}
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <p className="truncate text-sm font-medium">{c.subject || '(no subject)'}</p>
                <span className="bg-bg-secondary text-fg-muted md:text-tiny shrink-0 rounded-full px-1.5 py-0.5 text-xs">
                  {c.message_count}
                </span>
              </div>
              <p className="text-fg-secondary truncate text-xs">{c.participants}</p>
              <p className="text-fg-muted mt-0.5 truncate text-xs">{c.snippet}</p>
            </div>
            <span className="text-fg-muted shrink-0 text-xs">{formatDate(c.last_date)}</span>
          </button>
        ))}
        {!isFetching && conversations.length === 0 && <Note text="No conversations found" />}
      </div>
    </div>
  )
}

export function AuditThread({
  account,
  onBack,
  threadId,
}: {
  account: string
  onBack: () => void
  threadId: string
}) {
  const { data, isFetching } = useQuery({
    queryKey: adminKeys.mailAuditThread(account, threadId),
    queryFn: ({ signal }) =>
      adminListGet<AuditMessage>(
        `/admin/audit/conversations/${enc(threadId)}/messages?user=${enc(account)}`,
        signal
      ),
  })
  const messages = data ?? NO_MESSAGES

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <AuditHeader
        backLabel="Back to conversations"
        onBack={onBack}
        title={messages[0]?.subject || threadId}
        warning={`Audit Mode — ${account}`}
      />
      <div className="flex-1 overflow-y-auto px-6">
        {isFetching && messages.length === 0 && <Note text="Loading…" />}
        {messages.map((msg) => (
          <MessageView key={msg.uid} msg={msg} targetUser={account} />
        ))}
        {!isFetching && messages.length === 0 && <Note text="No messages" />}
      </div>
    </div>
  )
}

function AuditHeader({
  backLabel,
  onBack,
  title,
  warning = 'Audit Mode',
}: {
  backLabel: string
  onBack: () => void
  title: string
  warning?: string
}) {
  return (
    <div className="border-border flex items-center gap-3 border-b px-6 py-3">
      <button
        aria-label={backLabel}
        className="text-fg-muted hover:bg-bg-secondary rounded-md p-1 transition-colors"
        onClick={onBack}
      >
        <ChevronLeft className="h-5 w-5" />
      </button>
      <div className="min-w-0 flex-1">
        <p className="text-warning text-xs">{warning}</p>
        <p className="truncate text-sm font-medium">{title}</p>
      </div>
    </div>
  )
}

function formatDate(epoch: number): string {
  const d = new Date(epoch * 1000)
  const now = new Date()
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }
  return d.toLocaleDateString([], { day: 'numeric', month: 'short' })
}

function formatFullDate(epoch: number): string {
  return new Date(epoch * 1000).toLocaleString()
}

function MessageBody({ msg }: { msg: AuditMessage }) {
  if (msg.html_body) return <HtmlFrame html={msg.html_body} />
  if (msg.text_body) {
    return (
      <pre className="text-fg-secondary max-h-96 overflow-auto p-3 text-sm whitespace-pre-wrap">
        {msg.text_body}
      </pre>
    )
  }
  return <p className="text-fg-muted p-3 text-sm">(no content)</p>
}

function MessageView({ msg, targetUser }: { msg: AuditMessage; targetUser: string }) {
  const token = getToken() ?? ''
  const attachments = msg.attachments ?? []

  return (
    <div className="border-border border-b py-4">
      <div className="mb-2 flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">{msg.sender}</p>
          <p className="text-fg-muted truncate text-xs">To: {msg.recipients}</p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <span className="text-fg-muted text-xs">{formatFullDate(msg.internal_date)}</span>
          <a
            aria-label="Download as .eml"
            className="text-fg-muted hover:bg-bg-secondary rounded-md p-1 transition-colors"
            href={`/api/admin/audit/messages/${msg.uid}/raw?user=${enc(targetUser)}&token=${enc(token)}`}
            title="Download .eml"
          >
            <Download className="h-3.5 w-3.5" />
          </a>
        </div>
      </div>

      {msg.risk_score > 0 && (
        <div className="bg-danger/10 text-danger mb-2 rounded-md px-2 py-1 text-xs">
          Risk score: {msg.risk_score}
        </div>
      )}

      <div className="border-border rounded-lg border bg-white">
        <MessageBody msg={msg} />
      </div>

      {attachments.length > 0 && (
        <div className="text-fg-secondary mt-2 flex flex-wrap items-center gap-2 text-xs">
          {attachments.map((att, i) => (
            <span className="border-border rounded-md border px-2 py-1" key={i}>
              {att.filename} ({(att.size / 1024).toFixed(0)} KB)
            </span>
          ))}
          <span className="text-fg-muted">Attachments are in the .eml download</span>
        </div>
      )}
    </div>
  )
}

function Note({ text }: { text: string }) {
  return <p className="text-fg-muted py-8 text-center text-sm">{text}</p>
}
