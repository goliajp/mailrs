import { useQuery } from '@tanstack/react-query'
import { Inbox } from 'lucide-react'

import { BackButton } from '@/components/admin-mail-audit/back-button'
import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
} from '@/components/admin-page'
import { formatDate, formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { adminListGet } from '@/wire/endpoints/admin'

export { AuditThread } from '@/components/admin-mail-audit/audit-thread'

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

const enc = encodeURIComponent
const HEADERS = ['Conversation', 'Participants', 'Messages', 'Last message', '']
const NO_CONVERSATIONS: AuditConversation[] = []

/** Another account's newest conversations. Listing them writes an `audit.list_conversations` row. */
export function AuditConversations({
  account,
  onBack,
  onOpen,
}: {
  account: string
  onBack: () => void
  onOpen: (threadId: string) => void
}) {
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: adminKeys.mailAuditConversations(account),
    queryFn: ({ signal }) =>
      adminListGet<AuditConversation>(
        `/admin/audit/conversations?user=${enc(account)}&limit=50`,
        signal
      ),
  })
  const conversations = data ?? NO_CONVERSATIONS

  const columns: Column<AuditConversation>[] = [
    {
      header: 'Conversation',
      key: 'subject',
      cell: (c) => (
        <div className="max-w-md min-w-0">
          <p className="truncate font-medium">{c.subject || '(no subject)'}</p>
          <p className="text-fg-muted truncate text-xs">{c.snippet}</p>
        </div>
      ),
    },
    {
      header: 'Participants',
      key: 'participants',
      cell: (c) => (
        <span className="text-fg-secondary block max-w-xs truncate text-xs">{c.participants}</span>
      ),
    },
    { align: 'end', header: 'Messages', key: 'count', cell: (c) => c.message_count },
    {
      align: 'end',
      header: 'Last message',
      key: 'last',
      cell: (c) => (
        <span className="text-fg-secondary whitespace-nowrap" title={formatFullDate(c.last_date)}>
          {formatDate(c.last_date)}
        </span>
      ),
    },
    {
      align: 'end',
      header: 'Actions',
      hideHeader: true,
      key: 'actions',
      cell: (c) => (
        <button
          className="text-accent text-xs hover:opacity-80"
          onClick={() => onOpen(c.thread_id)}
          type="button"
        >
          Open
        </button>
      ),
    },
  ]

  const renderBody = () => {
    if (isPending) return <AdminTableSkeleton cols={5} headers={HEADERS} rows={6} />
    if (error) {
      return (
        <AdminErrorState
          error={error}
          onRetry={() => refetch()}
          retryDisabled={isFetching}
          what={`${account}'s conversations`}
        />
      )
    }
    if (conversations.length === 0) {
      return (
        <AdminEmptyState
          description={`Mail ${account} sends or receives will be listed here`}
          icon={<Inbox className="h-10 w-10" />}
          title="No conversations yet"
        />
      )
    }
    return (
      <AdminTable
        columns={columns}
        label={`${account}'s conversations`}
        rowKey={(c) => c.thread_id}
        rows={conversations}
      />
    )
  }

  return (
    <AdminPageShell
      actions={<BackButton label="Back to accounts" onBack={onBack} />}
      subtitle="Up to 50 conversations. Each time this list or a conversation is opened, the audit log records it under your address"
      title={account}
    >
      {renderBody()}
    </AdminPageShell>
  )
}
