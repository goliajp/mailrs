import { useEffect, useRef, useState } from 'react'

import { useSmtpEvents } from '@/hooks/use-smtp-events'
import { formatUptime } from '@/lib/format'
import type { ConnectionInfo, SmtpEvent } from '@/lib/types'

function StatusCard({
  label,
  value,
  color,
}: {
  label: string
  value: string | number
  color: string
}) {
  return (
    <div className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-3">
      <div className="text-xs text-[var(--color-text-tertiary)]">{label}</div>
      <div className={`mt-1 text-2xl font-bold tabular-nums ${color}`}>
        {value}
      </div>
    </div>
  )
}

function ConnectionRow({
  conn,
  selected,
  onClick,
}: {
  conn: ConnectionInfo
  selected: boolean
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      className={`flex w-full items-center gap-3 rounded px-3 py-2 text-left text-sm transition-colors ${
        selected
          ? 'bg-[var(--color-bg-selected)] text-[var(--color-text-primary)]'
          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
      }`}
    >
      <span className="font-mono text-xs text-[var(--color-text-tertiary)]">
        #{conn.id}
      </span>
      <span className="truncate">{conn.addr}</span>
      <div className="ml-auto flex items-center gap-1.5">
        {conn.tls && (
          <span className="rounded-md bg-[var(--color-status-success-subtle)] px-1.5 py-0.5 text-xs text-[var(--color-status-success)]">
            TLS
          </span>
        )}
        {conn.authenticated && (
          <span className="rounded-md bg-[var(--color-brand-subtle)] px-1.5 py-0.5 text-xs text-[var(--color-brand-primary)]">
            {conn.authenticated}
          </span>
        )}
        <span className="rounded bg-[var(--color-bg-sunken)] px-1.5 py-0.5 text-xs text-[var(--color-text-tertiary)]">
          {conn.state}
        </span>
      </div>
    </button>
  )
}

function ConversationView({ conn }: { conn: ConnectionInfo | null }) {
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    scrollRef.current?.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: 'smooth',
    })
  }, [conn?.lines.length])

  if (!conn) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[var(--color-text-tertiary)]">
        select a connection to view its SMTP conversation
      </div>
    )
  }

  return (
    <div ref={scrollRef} className="h-full space-y-2 overflow-y-auto p-4">
      {conn.lines.map((line, i) => {
        const isServer = line.direction === 'server'
        return (
          <div
            key={i}
            className={`flex gap-2 ${isServer ? '' : 'flex-row-reverse'}`}
          >
            <div
              className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-xs font-bold ${
                isServer
                  ? 'bg-[var(--color-status-success-subtle)] text-[var(--color-status-success)]'
                  : 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'
              }`}
            >
              {isServer ? 'S' : 'C'}
            </div>
            <div
              className={`max-w-[80%] rounded-md px-3 py-1.5 font-mono text-xs leading-relaxed ${
                isServer
                  ? 'bg-[var(--color-bg-sunken)] text-[var(--color-status-success)]'
                  : 'bg-[var(--color-bg-sunken)] text-[var(--color-brand-primary)]'
              }`}
            >
              {line.text.split('\r\n').map((l, j) => (
                <div key={j}>{l || '\u00A0'}</div>
              ))}
            </div>
          </div>
        )
      })}
      {conn.lines.length === 0 && (
        <div className="text-sm text-[var(--color-text-tertiary)]">
          waiting for commands...
        </div>
      )}
    </div>
  )
}

function EventLog({ events }: { events: SmtpEvent[] }) {
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    scrollRef.current?.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: 'smooth',
    })
  }, [events.length])

  return (
    <div ref={scrollRef} className="h-full space-y-1 overflow-y-auto p-3">
      {events
        .slice()
        .reverse()
        .slice(0, 100)
        .map((event, i) => (
          <div
            key={i}
            className="flex items-center gap-2 rounded-md px-2 py-1 font-mono text-xs"
          >
            <span className="text-[var(--color-text-tertiary)]">
              #{event.id}
            </span>
            <EventBadge type={event.type} />
            <span className="truncate text-[var(--color-text-tertiary)]">
              {formatEvent(event)}
            </span>
          </div>
        ))}
      {events.length === 0 && (
        <div className="text-xs text-[var(--color-text-tertiary)]">
          no events yet — connect to the SMTP server to see real-time activity
        </div>
      )}
    </div>
  )
}

function EventBadge({ type }: { type: string }) {
  const colors: Record<string, string> = {
    ConnectionOpened: 'text-[var(--color-status-success)]',
    ConnectionClosed: 'text-[var(--color-status-danger)]',
    CommandReceived: 'text-[var(--color-brand-primary)]',
    ResponseSent: 'text-[var(--color-status-warning)]',
    TlsUpgraded: 'text-[var(--color-brand-primary)]',
    Authenticated: 'text-[var(--color-status-info)]',
    MessageDelivered: 'text-[var(--color-status-success)]',
  }

  return (
    <span className={`shrink-0 ${colors[type] ?? 'text-[var(--color-text-tertiary)]'}`}>
      {type.replace(/([A-Z])/g, ' $1').trim()}
    </span>
  )
}

function formatEvent(event: SmtpEvent): string {
  switch (event.type) {
    case 'ConnectionOpened':
      return `${event.addr}${event.tls ? ' (TLS)' : ''}`
    case 'ConnectionClosed':
      return 'closed'
    case 'CommandReceived':
      return event.command
    case 'ResponseSent':
      return event.response.split('\r\n')[0] ?? ''
    case 'TlsUpgraded':
      return 'TLS handshake complete'
    case 'Authenticated':
      return event.username
    case 'MessageDelivered':
      return `${event.from} → ${event.to.join(', ')} (${event.size}B)`
    case 'SpamRejected':
      return event.reason
    case 'MessageQueued':
      return `${event.from} → ${event.to.join(', ')}`
    default:
      return ''
  }
}

export function Protocol() {
  const { connections, events, status, connected } = useSmtpEvents()
  const [selectedId, setSelectedId] = useState<number | null>(null)

  const connList = Array.from(connections.values())
  const selectedConn =
    selectedId !== null ? (connections.get(selectedId) ?? null) : null

  return (
    <div className="flex h-full flex-col">
      {/* header */}
      <div className="flex items-center justify-between border-b border-[var(--color-border-default)] px-6 py-4">
        <div>
          <h1 className="text-lg font-semibold">SMTP Live Monitor</h1>
          <p className="mt-0.5 text-sm text-[var(--color-text-tertiary)]">
            real-time SMTP session status and event stream
          </p>
        </div>
        <div className="flex items-center gap-2">
          <div
            className={`h-2.5 w-2.5 rounded-full ${connected ? 'bg-[var(--color-status-success)]' : 'bg-[var(--color-status-danger)]'}`}
          />
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {connected ? 'connected' : 'disconnected'}
          </span>
        </div>
      </div>

      {/* status cards */}
      <div className="grid grid-cols-4 gap-3 border-b border-[var(--color-border-default)] px-6 py-3">
        <StatusCard
          label="Active Connections"
          value={status?.active_connections ?? 0}
          color="text-[var(--color-status-success)]"
        />
        <StatusCard
          label="Total Connections"
          value={status?.total_connections ?? 0}
          color="text-[var(--color-brand-primary)]"
        />
        <StatusCard
          label="Messages Delivered"
          value={status?.total_messages ?? 0}
          color="text-[var(--color-status-warning)]"
        />
        <StatusCard
          label="Uptime"
          value={status ? formatUptime(status.uptime_secs) : '-'}
          color="text-[var(--color-text-secondary)]"
        />
      </div>

      <div className="flex flex-1 overflow-hidden">
        {/* left: connection list */}
        <div className="flex w-72 shrink-0 flex-col border-r border-[var(--color-border-default)]">
          <div className="border-b border-[var(--color-border-default)] px-4 py-2 text-xs font-medium tracking-wider text-[var(--color-text-tertiary)] uppercase">
            Active Sessions ({connList.length})
          </div>
          <div className="flex-1 space-y-1 overflow-y-auto p-2">
            {connList.map((conn) => (
              <ConnectionRow
                key={conn.id}
                conn={conn}
                selected={conn.id === selectedId}
                onClick={() => setSelectedId(conn.id)}
              />
            ))}
            {connList.length === 0 && (
              <div className="p-4 text-center text-xs text-[var(--color-text-tertiary)]">
                no active connections
              </div>
            )}
          </div>
        </div>

        {/* center: conversation */}
        <div className="flex flex-1 flex-col">
          <div className="border-b border-[var(--color-border-default)] px-4 py-2 text-xs font-medium tracking-wider text-[var(--color-text-tertiary)] uppercase">
            {selectedConn
              ? `Session #${selectedConn.id} — ${selectedConn.addr}`
              : 'Conversation'}
          </div>
          <div className="flex-1 overflow-hidden">
            <ConversationView conn={selectedConn} />
          </div>
        </div>

        {/* right: event log */}
        <div className="flex w-96 shrink-0 flex-col border-l border-[var(--color-border-default)]">
          <div className="border-b border-[var(--color-border-default)] px-4 py-2 text-xs font-medium tracking-wider text-[var(--color-text-tertiary)] uppercase">
            Event Stream ({events.length})
          </div>
          <div className="flex-1 overflow-hidden">
            <EventLog events={events} />
          </div>
        </div>
      </div>
    </div>
  )
}
