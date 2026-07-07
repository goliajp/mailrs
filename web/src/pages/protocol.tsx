import type { ConnectionInfo, SmtpEvent } from '@/lib/types'

import { useEffect, useRef, useState } from 'react'

import { useSmtpEvents } from '@/hooks/use-smtp-events'
import { formatUptime } from '@/lib/format'

// Format a counter that may legitimately be `null` from the backend.
// A `null` here means "this webapi process does not observe that
// counter" (see /api/status handler); render `-` so users can tell
// "unknown" apart from "zero".
function fmt(n: null | number | undefined): string {
  if (n == null) return '-'
  return n.toLocaleString('en-US')
}

const EVENT_COLORS: Record<string, string> = {
  Authenticated: 'text-info',
  CommandReceived: 'text-accent',
  ConnectionClosed: 'text-danger',
  ConnectionOpened: 'text-success',
  MessageDelivered: 'text-success',
  ResponseSent: 'text-warning',
  TlsUpgraded: 'text-accent',
}

export function Protocol() {
  const { connected, connections, events, status } = useSmtpEvents()
  const [selectedId, setSelectedId] = useState<null | number>(null)

  const connList = Array.from(connections.values())
  const selectedConn = selectedId !== null ? (connections.get(selectedId) ?? null) : null

  return (
    <main className="flex h-full flex-col">
      {/* header */}
      <div className="border-border flex items-center justify-between border-b px-4 py-3">
        <div>
          <h1 className="text-lg font-semibold">SMTP Live Monitor</h1>
          <p className="text-fg-muted mt-0.5 text-sm">
            real-time SMTP session status and event stream
          </p>
        </div>
        <div aria-live="polite" className="flex items-center gap-2" role="status">
          <div className={`h-2.5 w-2.5 rounded-full ${connected ? 'bg-success' : 'bg-danger'}`} />
          <span className="text-fg-muted text-xs">{connected ? 'connected' : 'disconnected'}</span>
        </div>
      </div>

      {/* status cards. Backend nulls a counter when this webapi process
       * can't observe it directly (fastcore split: SMTP counters live in
       * mailrs-receiver, not here). Show `-` for those instead of a
       * misleading zero. */}
      <div className="border-border grid grid-cols-2 gap-3 border-b px-6 py-3 md:grid-cols-4">
        <StatusCard
          color="text-success"
          label="Active Connections"
          value={fmt(status?.active_connections)}
        />
        <StatusCard
          color="text-accent"
          label="Total Connections"
          value={fmt(status?.total_connections)}
        />
        <StatusCard
          color="text-warning"
          label="Messages Delivered"
          value={fmt(status?.total_messages)}
        />
        <StatusCard
          color="text-fg-secondary"
          label="Uptime"
          value={formatUptime(status?.uptime_secs)}
        />
      </div>

      <div className="flex flex-1 overflow-hidden">
        {/* left: connection list */}
        <section
          aria-label="Active SMTP sessions"
          className="border-border flex w-72 shrink-0 flex-col border-r"
        >
          <div className="border-border text-fg-muted border-b px-4 py-2 text-xs font-medium tracking-wider uppercase">
            Active Sessions ({connList.length})
          </div>
          <div className="flex-1 space-y-1 overflow-y-auto p-2">
            {connList.map((conn) => (
              <ConnectionRow
                conn={conn}
                key={conn.id}
                onClick={() => setSelectedId(conn.id)}
                selected={conn.id === selectedId}
              />
            ))}
            {connList.length === 0 && (
              <div className="text-fg-muted p-4 text-center text-xs">no active connections</div>
            )}
          </div>
        </section>

        {/* center: conversation */}
        <section aria-label="Session conversation" className="flex flex-1 flex-col">
          <div className="border-border text-fg-muted border-b px-4 py-2 text-xs font-medium tracking-wider uppercase">
            {selectedConn ? `Session #${selectedConn.id} — ${selectedConn.addr}` : 'Conversation'}
          </div>
          <div className="flex-1 overflow-hidden">
            <ConversationView conn={selectedConn} />
          </div>
        </section>

        {/* right: event log */}
        <section
          aria-label="Event stream"
          className="border-border flex w-96 shrink-0 flex-col border-l"
        >
          <div className="border-border text-fg-muted border-b px-4 py-2 text-xs font-medium tracking-wider uppercase">
            Event Stream ({events.length})
          </div>
          <div className="flex-1 overflow-hidden">
            <EventLog events={events} />
          </div>
        </section>
      </div>
    </main>
  )
}

function ConnectionRow({
  conn,
  onClick,
  selected,
}: {
  conn: ConnectionInfo
  onClick: () => void
  selected: boolean
}) {
  return (
    <button
      aria-pressed={selected}
      className={`flex w-full items-center gap-3 rounded px-3 py-2 text-left text-sm transition-colors ${
        selected ? 'bg-accent/20 text-fg' : 'text-fg-secondary hover:bg-bg-secondary'
      }`}
      onClick={onClick}
      type="button"
    >
      <span className="text-fg-muted font-mono text-xs">#{conn.id}</span>
      <span className="truncate">{conn.addr}</span>
      <div className="ml-auto flex items-center gap-1.5">
        {conn.tls && (
          <span className="bg-success/10 text-success rounded-md px-1.5 py-0.5 text-xs">TLS</span>
        )}
        {conn.authenticated && (
          <span className="bg-accent/10 text-accent rounded-md px-1.5 py-0.5 text-xs">
            {conn.authenticated}
          </span>
        )}
        <span className="bg-bg-secondary text-fg-muted rounded px-1.5 py-0.5 text-xs">
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
      behavior: 'smooth',
      top: scrollRef.current.scrollHeight,
    })
  }, [conn?.lines.length])

  if (!conn) {
    return (
      <div className="text-fg-muted flex h-full items-center justify-center text-sm">
        select a connection to view its SMTP conversation
      </div>
    )
  }

  return (
    <div className="h-full space-y-2 overflow-y-auto p-4" ref={scrollRef}>
      {conn.lines.map((line, i) => {
        const isServer = line.direction === 'server'
        return (
          <div className={`flex gap-2 ${isServer ? '' : 'flex-row-reverse'}`} key={i}>
            <div
              aria-label={isServer ? 'Server' : 'Client'}
              className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-xs font-bold ${
                isServer ? 'bg-success/10 text-success' : 'bg-accent/10 text-accent'
              }`}
            >
              {isServer ? 'S' : 'C'}
            </div>
            <div
              className={`max-w-[80%] rounded-md px-3 py-1.5 font-mono text-xs leading-relaxed ${
                isServer ? 'bg-bg-secondary text-success' : 'bg-bg-secondary text-accent'
              }`}
            >
              {line.text.split('\r\n').map((l, j) => (
                <div key={j}>{l || ' '}</div>
              ))}
            </div>
          </div>
        )
      })}
      {conn.lines.length === 0 && (
        <div className="text-fg-muted text-sm">waiting for commands...</div>
      )}
    </div>
  )
}

function EventBadge({ type }: { type: string }) {
  return (
    <span className={`shrink-0 ${EVENT_COLORS[type] ?? 'text-fg-muted'}`}>
      {type.replace(/([A-Z])/g, ' $1').trim()}
    </span>
  )
}

function EventLog({ events }: { events: SmtpEvent[] }) {
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    scrollRef.current?.scrollTo({
      behavior: 'smooth',
      top: scrollRef.current.scrollHeight,
    })
  }, [events.length])

  return (
    <div className="h-full space-y-1 overflow-y-auto p-3" ref={scrollRef}>
      {events
        .slice()
        .reverse()
        .slice(0, 100)
        .map((event, i) => (
          <div className="flex items-center gap-2 rounded-md px-2 py-1 font-mono text-xs" key={i}>
            <span className="text-fg-muted">#{event.id}</span>
            <EventBadge type={event.type} />
            <span className="text-fg-muted truncate">{formatEvent(event)}</span>
          </div>
        ))}
      {events.length === 0 && (
        <div className="text-fg-muted text-xs">
          no events yet — connect to the SMTP server to see real-time activity
        </div>
      )}
    </div>
  )
}

function formatEvent(event: SmtpEvent): string {
  switch (event.type) {
    case 'Authenticated':
      return event.username
    case 'CommandReceived':
      return event.command
    case 'ConnectionClosed':
      return 'closed'
    case 'ConnectionOpened':
      return `${event.addr}${event.tls ? ' (TLS)' : ''}`
    case 'MessageDelivered':
      return `${event.from} → ${event.to.join(', ')} (${event.size}B)`
    case 'MessageQueued':
      return `${event.from} → ${event.to.join(', ')}`
    case 'ResponseSent':
      return event.response.split('\r\n')[0] ?? ''
    case 'SpamRejected':
      return event.reason
    case 'TlsUpgraded':
      return 'TLS handshake complete'
    default:
      return ''
  }
}

function StatusCard({
  color,
  label,
  value,
}: {
  color: string
  label: string
  value: number | string
}) {
  return (
    <div className="border-border bg-bg-secondary rounded-lg border p-3">
      <div className="text-fg-muted text-xs">{label}</div>
      <div className={`mt-1 text-2xl font-bold tabular-nums ${color}`}>{value}</div>
    </div>
  )
}
