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
    <div className="rounded border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900">
      <div className="text-xs text-zinc-500 dark:text-zinc-400">{label}</div>
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
          ? 'bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
          : 'text-zinc-600 hover:bg-zinc-50 dark:text-zinc-400 dark:hover:bg-zinc-800/50'
      }`}
    >
      <span className="font-mono text-xs text-zinc-400 dark:text-zinc-600">
        #{conn.id}
      </span>
      <span className="truncate">{conn.addr}</span>
      <div className="ml-auto flex items-center gap-1.5">
        {conn.tls && (
          <span className="rounded bg-emerald-100 px-1.5 py-0.5 text-xs text-emerald-700 dark:bg-emerald-900/50 dark:text-emerald-400">
            TLS
          </span>
        )}
        {conn.authenticated && (
          <span className="rounded bg-blue-100 px-1.5 py-0.5 text-xs text-blue-700 dark:bg-blue-900/50 dark:text-blue-400">
            {conn.authenticated}
          </span>
        )}
        <span className="rounded bg-zinc-100 px-1.5 py-0.5 text-xs text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400">
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
      <div className="flex h-full items-center justify-center text-sm text-zinc-400 dark:text-zinc-600">
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
                  ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900 dark:text-emerald-300'
                  : 'bg-blue-100 text-blue-700 dark:bg-blue-900 dark:text-blue-300'
              }`}
            >
              {isServer ? 'S' : 'C'}
            </div>
            <div
              className={`max-w-[80%] rounded px-3 py-1.5 font-mono text-xs leading-relaxed ${
                isServer
                  ? 'bg-zinc-100 text-emerald-700 dark:bg-zinc-800 dark:text-emerald-300'
                  : 'bg-zinc-100 text-blue-700 dark:bg-zinc-800 dark:text-blue-300'
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
        <div className="text-sm text-zinc-400 dark:text-zinc-600">
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
            className="flex items-center gap-2 rounded px-2 py-1 font-mono text-xs"
          >
            <span className="text-zinc-400 dark:text-zinc-600">
              #{event.id}
            </span>
            <EventBadge type={event.type} />
            <span className="truncate text-zinc-500 dark:text-zinc-400">
              {formatEvent(event)}
            </span>
          </div>
        ))}
      {events.length === 0 && (
        <div className="text-xs text-zinc-400 dark:text-zinc-600">
          no events yet — connect to the SMTP server to see real-time activity
        </div>
      )}
    </div>
  )
}

function EventBadge({ type }: { type: string }) {
  const colors: Record<string, string> = {
    ConnectionOpened: 'text-emerald-600 dark:text-emerald-400',
    ConnectionClosed: 'text-red-600 dark:text-red-400',
    CommandReceived: 'text-blue-600 dark:text-blue-400',
    ResponseSent: 'text-amber-600 dark:text-amber-400',
    TlsUpgraded: 'text-purple-600 dark:text-purple-400',
    Authenticated: 'text-cyan-600 dark:text-cyan-400',
    MessageDelivered: 'text-green-600 dark:text-green-400',
  }

  return (
    <span className={`shrink-0 ${colors[type] ?? 'text-zinc-500'}`}>
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
    <div className="flex h-screen flex-col bg-white text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      {/* header */}
      <div className="flex items-center justify-between border-b border-zinc-200 px-6 py-4 dark:border-zinc-800">
        <div>
          <h1 className="text-lg font-semibold">SMTP Live Monitor</h1>
          <p className="mt-0.5 text-sm text-zinc-500 dark:text-zinc-400">
            real-time SMTP session status and event stream
          </p>
        </div>
        <div className="flex items-center gap-2">
          <div
            className={`h-2.5 w-2.5 rounded-full ${connected ? 'bg-emerald-500' : 'bg-red-500'}`}
          />
          <span className="text-xs text-zinc-500 dark:text-zinc-400">
            {connected ? 'connected' : 'disconnected'}
          </span>
        </div>
      </div>

      {/* status cards */}
      <div className="grid grid-cols-4 gap-3 border-b border-zinc-200 px-6 py-3 dark:border-zinc-800">
        <StatusCard
          label="Active Connections"
          value={status?.active_connections ?? 0}
          color="text-emerald-600 dark:text-emerald-400"
        />
        <StatusCard
          label="Total Connections"
          value={status?.total_connections ?? 0}
          color="text-blue-600 dark:text-blue-400"
        />
        <StatusCard
          label="Messages Delivered"
          value={status?.total_messages ?? 0}
          color="text-amber-600 dark:text-amber-400"
        />
        <StatusCard
          label="Uptime"
          value={status ? formatUptime(status.uptime_secs) : '-'}
          color="text-zinc-700 dark:text-zinc-300"
        />
      </div>

      <div className="flex flex-1 overflow-hidden">
        {/* left: connection list */}
        <div className="flex w-72 shrink-0 flex-col border-r border-zinc-200 dark:border-zinc-800">
          <div className="border-b border-zinc-200 px-4 py-2 text-xs font-medium tracking-wider text-zinc-500 uppercase dark:border-zinc-800 dark:text-zinc-400">
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
              <div className="p-4 text-center text-xs text-zinc-400 dark:text-zinc-600">
                no active connections
              </div>
            )}
          </div>
        </div>

        {/* center: conversation */}
        <div className="flex flex-1 flex-col">
          <div className="border-b border-zinc-200 px-4 py-2 text-xs font-medium tracking-wider text-zinc-500 uppercase dark:border-zinc-800 dark:text-zinc-400">
            {selectedConn
              ? `Session #${selectedConn.id} — ${selectedConn.addr}`
              : 'Conversation'}
          </div>
          <div className="flex-1 overflow-hidden">
            <ConversationView conn={selectedConn} />
          </div>
        </div>

        {/* right: event log */}
        <div className="flex w-96 shrink-0 flex-col border-l border-zinc-200 dark:border-zinc-800">
          <div className="border-b border-zinc-200 px-4 py-2 text-xs font-medium tracking-wider text-zinc-500 uppercase dark:border-zinc-800 dark:text-zinc-400">
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
