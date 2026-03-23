import type { AttachmentInfo } from '@/lib/types'

import { ChevronLeft, Download, Eye, Search } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

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
      (a) =>
        a.address.toLowerCase().includes(q) ||
        a.display_name.toLowerCase().includes(q)
    )
  }, [accounts, search])

  // no account selected: show account list
  if (!selectedAccount) {
    return (
      <div className="flex-1 overflow-y-auto p-6">
        <div className="mb-6">
          <div className="mb-1 flex items-center gap-2">
            <Eye className="h-5 w-5 text-[var(--color-text-tertiary)]" />
            <h2 className="text-lg font-semibold">Mail Audit</h2>
          </div>
          <p className="text-sm text-[var(--color-text-tertiary)]">
            Select an account to review their email conversations
          </p>
        </div>

        <div className="mb-4 flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-[var(--color-text-tertiary)]" />
            <input
              className="w-full rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-primary)] py-2 pr-3 pl-9 text-sm outline-none focus:border-[var(--color-border-focus)]"
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search accounts..."
              type="text"
              value={search}
            />
          </div>
        </div>

        <div className="overflow-hidden rounded-lg border border-[var(--color-border-default)]">
          <table className="w-full text-left text-sm">
            <thead className="border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]">
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
                  className="border-b border-[var(--color-border-default)] last:border-0 hover:bg-[var(--color-hover)]"
                  key={a.address}
                >
                  <td className="px-4 py-3 font-medium">{a.address}</td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {a.domain}
                  </td>
                  <td className="px-4 py-3 text-[var(--color-text-secondary)]">
                    {a.display_name || '—'}
                  </td>
                  <td className="px-4 py-3">
                    <span
                      className={`rounded-full px-2 py-0.5 text-xs font-medium ${a.active ? 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]' : 'bg-[var(--color-bg-sunken)] text-[var(--color-text-tertiary)]'}`}
                    >
                      {a.active ? 'Active' : 'Inactive'}
                    </span>
                  </td>
                  <td className="px-4 py-3">
                    <button
                      className="rounded-md bg-[var(--color-bg-inverted)] px-3 py-1 text-xs font-medium text-[var(--color-text-on-inverted)] transition-colors hover:opacity-90"
                      onClick={() => handleSelectAccount(a.address)}
                    >
                      View Mail
                    </button>
                  </td>
                </tr>
              ))}
              {filteredAccounts.length === 0 && (
                <tr>
                  <td
                    className="px-4 py-8 text-center text-[var(--color-text-tertiary)]"
                    colSpan={5}
                  >
                    {accounts.length === 0
                      ? 'No auditable accounts (requires admin.impersonate permission)'
                      : 'No matches'}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    )
  }

  // thread selected: show messages
  if (selectedThread) {
    return (
      <div className="flex h-full flex-col overflow-hidden">
        <div className="flex items-center gap-3 border-b border-[var(--color-border-default)] px-6 py-3">
          <button
            className="rounded-md p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
            onClick={() => {
              setSelectedThread(null)
              setMessages([])
            }}
          >
            <ChevronLeft className="h-5 w-5" />
          </button>
          <div className="min-w-0 flex-1">
            <p className="text-xs text-[var(--color-status-warning)]">
              Audit Mode — {selectedAccount}
            </p>
            <p className="truncate text-sm font-medium">
              {messages[0]?.subject || selectedThread}
            </p>
          </div>
        </div>
        <div className="flex-1 overflow-y-auto px-6">
          {loading && (
            <p className="py-8 text-center text-sm text-[var(--color-text-tertiary)]">
              Loading...
            </p>
          )}
          {messages.map((msg) => (
            <MessageView key={msg.id} msg={msg} targetUser={selectedAccount} />
          ))}
          {!loading && messages.length === 0 && (
            <p className="py-8 text-center text-sm text-[var(--color-text-tertiary)]">
              No messages
            </p>
          )}
        </div>
      </div>
    )
  }

  // account selected: show conversations
  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="flex items-center gap-3 border-b border-[var(--color-border-default)] px-6 py-3">
        <button
          className="rounded-md p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
          onClick={() => {
            setSelectedAccount(null)
            setConversations([])
          }}
        >
          <ChevronLeft className="h-5 w-5" />
        </button>
        <div>
          <p className="text-xs text-[var(--color-status-warning)]">
            Audit Mode
          </p>
          <p className="text-sm font-medium">{selectedAccount}</p>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {loading && (
          <p className="py-8 text-center text-sm text-[var(--color-text-tertiary)]">
            Loading...
          </p>
        )}
        {conversations.map((c) => (
          <button
            className="flex w-full items-start gap-3 border-b border-[var(--color-border-default)] px-6 py-3 text-left transition-colors hover:bg-[var(--color-hover)]"
            key={c.thread_id}
            onClick={() => loadThread(c.thread_id)}
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <p className="truncate text-sm font-medium">
                  {c.subject || '(no subject)'}
                </p>
                <span className="shrink-0 rounded-full bg-[var(--color-bg-sunken)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-tertiary)]">
                  {c.message_count}
                </span>
              </div>
              <p className="truncate text-xs text-[var(--color-text-secondary)]">
                {c.participants.join(', ')}
              </p>
              <p className="mt-0.5 truncate text-xs text-[var(--color-text-tertiary)]">
                {c.snippet}
              </p>
            </div>
            <span className="shrink-0 text-xs text-[var(--color-text-tertiary)]">
              {formatDate(c.last_date)}
            </span>
          </button>
        ))}
        {!loading && conversations.length === 0 && (
          <p className="py-8 text-center text-sm text-[var(--color-text-tertiary)]">
            No conversations found
          </p>
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

function HtmlFrame({ html }: { html: string }) {
  const ref = useRef<HTMLIFrameElement>(null)
  const [height, setHeight] = useState(200)

  const srcdoc = useMemo(() => {
    const sanitized = sanitizeEmail(html)
    return `<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<style>
  body { margin: 0; padding: 12px; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; font-size: 14px; line-height: 1.6; color: #1a1a1a; background: #fff; word-wrap: break-word; overflow-wrap: break-word; }
  img { max-width: 100%; height: auto; }
  a { color: #0066cc; }
  table { max-width: 100%; }
  pre, code { white-space: pre-wrap; }
</style></head><body>${sanitized}</body></html>`
  }, [html])

  useEffect(() => {
    const iframe = ref.current
    if (!iframe) return
    const onLoad = () => {
      try {
        const h = iframe.contentDocument?.documentElement?.scrollHeight
        if (h && h > 50) setHeight(Math.min(h + 16, 800))
      } catch {
        // cross-origin
      }
    }
    iframe.addEventListener('load', onLoad)
    return () => iframe.removeEventListener('load', onLoad)
  }, [srcdoc])

  return (
    <iframe
      className="block w-full border-none"
      ref={ref}
      sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
      srcDoc={srcdoc}
      style={{ height }}
      title="email content"
    />
  )
}

function MessageView({
  msg,
  targetUser,
}: {
  msg: AuditMessage
  targetUser: string
}) {
  const token = getToken() ?? ''

  return (
    <div className="border-b border-[var(--color-border-default)] py-4">
      <div className="mb-2 flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">{msg.sender}</p>
          <p className="truncate text-xs text-[var(--color-text-tertiary)]">
            To: {msg.recipients}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {formatFullDate(msg.internal_date)}
          </span>
          <a
            className="rounded-md p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)]"
            href={`/api/admin/audit/messages/${msg.uid}/raw?target_user=${encodeURIComponent(targetUser)}&token=${encodeURIComponent(token)}`}
            title="Download .eml"
          >
            <Download className="h-3.5 w-3.5" />
          </a>
        </div>
      </div>

      {msg.risk_score > 0 && (
        <div className="mb-2 rounded-md bg-[var(--color-status-danger-subtle)] px-2 py-1 text-xs text-[var(--color-status-danger)]">
          Risk score: {msg.risk_score}
        </div>
      )}

      <div className="rounded-lg border border-[var(--color-border-default)] bg-white">
        {msg.html_body ? (
          <HtmlFrame html={msg.html_body} />
        ) : msg.text_body ? (
          <pre className="max-h-96 overflow-auto p-3 text-sm whitespace-pre-wrap text-[var(--color-text-secondary)]">
            {msg.text_body}
          </pre>
        ) : (
          <p className="p-3 text-sm text-[var(--color-text-tertiary)]">
            (no content)
          </p>
        )}
      </div>

      {msg.attachments.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-2">
          {msg.attachments.map((att, i) => (
            <a
              className="rounded-md border border-[var(--color-border-default)] px-2 py-1 text-xs text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
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

// sanitize html for safe iframe rendering
function sanitizeEmail(html: string): string {
  return html
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/on\w+\s*=\s*["'][^"']*["']/gi, '')
    .replace(/on\w+\s*=\s*\S+/gi, '')
}
