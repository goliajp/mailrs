import type { AttachmentInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'

import { AdminErrorState, AdminPageShell } from '@/components/admin-page'
import { HtmlFrame } from '@/components/html-frame'
import { useT } from '@/i18n'
import { formatFullDate, formatSize } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { adminListGet } from '@/wire/endpoints/admin'

import { BackButton } from './back-button'

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
const NO_MESSAGES: AuditMessage[] = []

/** Another account's thread. Opening it writes an `audit.read_thread` row. */
export function AuditThread({
  account,
  onBack,
  threadId,
}: {
  account: string
  onBack: () => void
  threadId: string
}) {
  const t = useT()
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: adminKeys.mailAuditThread(account, threadId),
    queryFn: ({ signal }) =>
      adminListGet<AuditMessage>(
        `/admin/audit/conversations/${enc(threadId)}/messages?user=${enc(account)}`,
        signal
      ),
  })
  const messages = data ?? NO_MESSAGES

  const renderBody = () => {
    if (isPending) {
      return <p className="text-fg-muted py-8 text-center text-sm">{t('common.loading')}</p>
    }
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          retryDisabled={isFetching}
          what={t('admin.mailAudit.thisConversation')}
        />
      )
    }
    if (messages.length === 0) {
      return (
        <p className="text-fg-muted py-8 text-center text-sm">{t('admin.mailAudit.noMessages')}</p>
      )
    }
    return messages.map((msg) => <MessageView key={msg.uid} msg={msg} targetUser={account} />)
  }

  return (
    <AdminPageShell
      actions={<BackButton label={t('admin.mailAudit.backToConversations')} onBack={onBack} />}
      subtitle={t('admin.mailAudit.threadSubtitle', { account })}
      title={messages[0]?.subject || t('admin.noSubject')}
    >
      {renderBody()}
    </AdminPageShell>
  )
}

function MessageBody({ msg }: { msg: AuditMessage }) {
  const t = useT()
  // html mail is drawn for a white page, as senders design it
  if (msg.html_body) {
    return (
      <div className="rounded-lg bg-white">
        <HtmlFrame html={msg.html_body} />
      </div>
    )
  }
  if (msg.text_body) {
    return (
      <pre className="text-fg max-h-96 overflow-auto p-3 font-sans text-sm whitespace-pre-wrap">
        {msg.text_body}
      </pre>
    )
  }
  return <p className="text-fg-muted p-3 text-sm">{t('admin.mailAudit.noBody')}</p>
}

function MessageView({ msg, targetUser }: { msg: AuditMessage; targetUser: string }) {
  const t = useT()
  const attachments = msg.attachments ?? []

  return (
    <div className="border-border border-b py-4 last:border-0">
      <div className="mb-2 flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">{msg.sender}</p>
          <p className="text-fg-muted truncate text-xs">
            {t('admin.mailAudit.to', { recipients: msg.recipients })}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-3 text-xs">
          <span className="text-fg-muted tabular-nums">{formatFullDate(msg.internal_date)}</span>
          <a
            aria-label={t('admin.mailAudit.downloadLabel')}
            className="text-accent hover:opacity-80"
            href={`/api/admin/audit/messages/${msg.uid}/raw?user=${enc(targetUser)}`}
            title={t('admin.mailAudit.downloadRecorded')}
          >
            {t('admin.mailAudit.download')}
          </a>
        </div>
      </div>

      {msg.risk_score > 0 && (
        <p className="text-danger mb-2 text-xs">
          {t('admin.mailAudit.risk', { score: msg.risk_score })}
        </p>
      )}

      <div className="border-border rounded-lg border">
        <MessageBody msg={msg} />
      </div>

      {attachments.length > 0 && (
        <div className="text-fg-secondary mt-2 flex flex-wrap items-center gap-2 text-xs">
          {attachments.map((att, i) => (
            <span className="border-border rounded-md border px-2 py-1" key={i}>
              {att.filename} ({formatSize(att.size)})
            </span>
          ))}
          <span className="text-fg-muted">{t('admin.mailAudit.attachmentsInEml')}</span>
        </div>
      )}
    </div>
  )
}
