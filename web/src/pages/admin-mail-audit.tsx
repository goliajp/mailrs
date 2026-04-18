import type { AttachmentInfo } from '@/lib/types'

import { ChevronLeft, Download, Eye, Search } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { HtmlFrame } from '@/components/html-frame'
import { ScrollableTable } from '@/components/scrollable-table'
import { fetchJson } from '@/lib/api'
import { getToken } from '@/store/auth'

type AuditAccount = {
  active: boolean
  address: string
  display_name: string
  domain: string
}

type AuditConversation = {
  category: string
  last_date: number
  message_count: number
  participants: string[]
  snippet: string
  subject: string
  thread_id: string
  unread_count: number
}

type AuditMessage = {
  attachments: AttachmentInfo[]
  category: string
  flags: number
  html_body: null | string
  id: number
  internal_date: number
  recipients: string
  risk_score: number
  sender: string
  subject: string
  summary: string
  text_body: null | string
  uid: number
}

export function AdminMailAudit() {
  const [accounts, setAccounts] = useState<AuditAccount[]>([])
  const [selectedAccount, setSelectedAccount] = useState<null | string>(null)
  const [conversations, setConversations] = useState<AuditConversation[]>([])
  const [selectedThread, setSelectedThread] = useState<null | string>(null)
  const [messages, setMessages] = useState<AuditMessage[]>([])
  const [loading, setLoading] = useState(false)
  const [search, setSearch] = useState('')

  // load auditable accounts
  useEffect(() => {
    fetchJson<AuditAccount[]>('/admin/audit/accounts')
      .then(setAccounts)
      .catch(() => setAccounts([]))
  }, [])

  // load conversations for selected account
  const loadConversations = useCallback(async (address: string) => {
    setLoading(true)
    setSelectedThread(null)
    setMessages([])
    try {
      const data = await fetchJson<AuditConversation[]>(
        `/admin/audit/conversations?target_user=${encodeURIComponent(address)}&limit=50`
      )
      setConversations(Array.isArray(data) ? data : [])
    } catch {
      setConversations([])
    } finally {
      setLoading(false)
    }
  }, [])

  // load thread messages
  const loadThread = useCallback(
    async (threadId: string) => {
      if (!selectedAccount) return
      setLoading(true)
      try {
        const data = await fetchJson<AuditMessage[]>(
          `/admin/audit/conversations/${encodeURIComponent(threadId)}/messages?target_user=${encodeURIComponent(selectedAccount)}`
        )
        setMessages(Array.isArray(data) ? data : [])
        setSelectedThread(threadId)
      } catch {
        setMessages([])
      } finally {
        setLoading(false)
      }
    },
    [selectedAccount]
  )

  const handleSelectAccount = useCallback(
    (address: string) => {
      setSelectedAccount(address)
      loadConversations(address)
    },
    [loadConversations]
  )

  const filteredAccounts = useMemo(() => {
    if (!search) return accounts
    const q = search.toLowerCase()
    return accounts.filter(
      (a) => a.address.toLowerCase().includes(q) || a.display_name.toLowerCase().includes(q)
    )
  }, [accounts, search])

  // no account selected: show account list
  if (!selectedAccount) {
    return (
      <div className="flex-1 overflow-y-auto p-6">
        <div className="mb-6">
          <div className="mb-1 flex items-center gap-2">
            <Eye className="text-fg-muted h-5 w-5" />
            <h2 className="text-lg font-semibold">Mail Audit</h2>
          </div>
          <p className="text-fg-muted text-sm">
            Select an account to review their email conversations
          </p>
        </div>

        <div className="mb-4 flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="text-fg-muted absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2" />
            <input
              className="border-border bg-bg focus:border-accent w-full rounded-lg border py-2 pr-3 pl-9 text-sm outline-none"
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search accounts..."
              type="text"
              value={search}
            />
          </div>
        </div>

        <div className="border-border overflow-hidden rounded-lg border">
          <ScrollableTable>
            <table className="w-full text-left text-sm">
              <thead className="border-border bg-bg-secondary border-b">
                <tr>
                  <th className="px-4 py-2.5 font-medium">Account</th>
                  <th className="px-4 py-2.5 font-medium">Domain</th>
                  <th className="px-4 py-2.5 font-medium">Name</th>
                  <th className="px-4 py-2.5 font-medium">Status</th>
                  <th className="px-4 py-2.5 font-medium" />
                </tr>
              </thead>
              <tbody>
                {filteredAccounts.map((a) => (
                  <tr
                    className="border-border hover:bg-bg-secondary border-b last:border-0"
                    key={a.address}
                  >
                    <td className="px-4 py-3 font-medium">{a.address}</td>
                    <td className="text-fg-secondary px-4 py-3">{a.domain}</td>
                    <td className="text-fg-secondary px-4 py-3">{a.display_name || '—'}</td>
                    <td className="px-4 py-3">
                      <span
                        className={`rounded-full px-2 py-0.5 text-xs font-medium ${a.active ? 'bg-success/10 text-success' : 'bg-bg-secondary text-fg-muted'}`}
                      >
                        {a.active ? 'Active' : 'Inactive'}
                      </span>
                    </td>
                    <td className="px-4 py-3">
                      <button
                        className="bg-fg text-bg rounded-md px-3 py-1 text-xs font-medium transition-colors hover:opacity-90"
                        onClick={() => handleSelectAccount(a.address)}
                      >
                        View Mail
                      </button>
                    </td>
                  </tr>
                ))}
                {filteredAccounts.length === 0 && (
                  <tr>
                    <td className="text-fg-muted px-4 py-8 text-center" colSpan={5}>
                      {accounts.length === 0
                        ? 'No auditable accounts (requires admin.impersonate permission)'
                        : 'No matches'}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </ScrollableTable>
        </div>
      </div>
    )
  }

  // thread selected: show messages
  if (selectedThread) {
    return (
      <div className="flex h-full flex-col overflow-hidden">
        <div className="border-border flex items-center gap-3 border-b px-6 py-3">
          <button
            className="text-fg-muted hover:bg-bg-secondary rounded-md p-1 transition-colors"
            onClick={() => {
              setSelectedThread(null)
              setMessages([])
            }}
          >
            <ChevronLeft className="h-5 w-5" />
          </button>
          <div className="min-w-0 flex-1">
            <p className="text-warning text-xs">Audit Mode — {selectedAccount}</p>
            <p className="truncate text-sm font-medium">{messages[0]?.subject || selectedThread}</p>
          </div>
        </div>
        <div className="flex-1 overflow-y-auto px-6">
          {loading && <p className="text-fg-muted py-8 text-center text-sm">Loading...</p>}
          {messages.map((msg) => (
            <MessageView key={msg.id} msg={msg} targetUser={selectedAccount} />
          ))}
          {!loading && messages.length === 0 && (
            <p className="text-fg-muted py-8 text-center text-sm">No messages</p>
          )}
        </div>
      </div>
    )
  }

  // account selected: show conversations
  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="border-border flex items-center gap-3 border-b px-6 py-3">
        <button
          className="text-fg-muted hover:bg-bg-secondary rounded-md p-1 transition-colors"
          onClick={() => {
            setSelectedAccount(null)
            setConversations([])
          }}
        >
          <ChevronLeft className="h-5 w-5" />
        </button>
        <div>
          <p className="text-warning text-xs">Audit Mode</p>
          <p className="text-sm font-medium">{selectedAccount}</p>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {loading && <p className="text-fg-muted py-8 text-center text-sm">Loading...</p>}
        {conversations.map((c) => (
          <button
            className="border-border hover:bg-bg-secondary flex w-full items-start gap-3 border-b px-6 py-3 text-left transition-colors"
            key={c.thread_id}
            onClick={() => loadThread(c.thread_id)}
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <p className="truncate text-sm font-medium">{c.subject || '(no subject)'}</p>
                <span className="bg-bg-secondary text-fg-muted shrink-0 rounded-full px-1.5 py-0.5 text-xs md:text-[10px]">
                  {c.message_count}
                </span>
              </div>
              <p className="text-fg-secondary truncate text-xs">{c.participants.join(', ')}</p>
              <p className="text-fg-muted mt-0.5 truncate text-xs">{c.snippet}</p>
            </div>
            <span className="text-fg-muted shrink-0 text-xs">{formatDate(c.last_date)}</span>
          </button>
        ))}
        {!loading && conversations.length === 0 && (
          <p className="text-fg-muted py-8 text-center text-sm">No conversations found</p>
        )}
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

function MessageView({ msg, targetUser }: { msg: AuditMessage; targetUser: string }) {
  const token = getToken() ?? ''

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
            className="text-fg-muted hover:bg-bg-secondary rounded-md p-1 transition-colors"
            href={`/api/admin/audit/messages/${msg.uid}/raw?target_user=${encodeURIComponent(targetUser)}&token=${encodeURIComponent(token)}`}
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
        {msg.html_body ? (
          <HtmlFrame html={msg.html_body} />
        ) : msg.text_body ? (
          <pre className="text-fg-secondary max-h-96 overflow-auto p-3 text-sm whitespace-pre-wrap">
            {msg.text_body}
          </pre>
        ) : (
          <p className="text-fg-muted p-3 text-sm">(no content)</p>
        )}
      </div>

      {msg.attachments.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-2">
          {msg.attachments.map((att, i) => (
            <a
              className="border-border text-fg-secondary hover:bg-bg-secondary rounded-md border px-2 py-1 text-xs transition-colors"
              href={`/api/mail/messages/${msg.uid}/attachments/${i}?token=${encodeURIComponent(token)}`}
              key={i}
              rel="noopener noreferrer"
              target="_blank"
            >
              {att.filename} ({(att.size / 1024).toFixed(0)} KB)
            </a>
          ))}
        </div>
      )}
    </div>
  )
}
