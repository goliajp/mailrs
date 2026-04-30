import { Calendar, Check, Clock, MapPin, Users, X } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { fetchJson, postJson } from '@/lib/api'

type Attendee = {
  cn: null | string
  email: string
  partstat: string
  role: string
  rsvp: boolean
}

// Mirrors crates/server/src/ical/mod.rs ParsedInvite. Kept loose (Record-of-string)
// because mailrs::ical can grow new fields and we don't want a hard couple.
type CalDateTime =
  | string // chrono ISO when unknown kind
  | { iso?: string; kind: 'date' }
  | { iso?: string; kind: 'floating' }
  | { iso?: string; kind: 'utc' }
  | { iso?: string; kind: 'zoned'; tz?: string }

type ConflictRow = {
  dtend: null | string
  dtstart: null | string
  organizer: null | string
  status: null | string
  summary: string
  uid: string
}

type InvitePayload = {
  attendees?: Attendee[]
  description?: null | string
  dtend?: CalDateTime | null
  dtstart?: CalDateTime
  location?: null | string
  method: string
  organizer?: null | Person
  rrule?: null | string
  sequence: number
  status?: null | string
  summary: string
  uid: string
}

type MessageDetail = {
  invite_method: null | string
  invite_payload: InvitePayload | null
  uid: number
}

type Person = { cn: null | string; email: string }

type RsvpStatus = 'idle' | 'pending' | 'sent' | { error: string }

export function InviteCard({ messageUid }: { messageUid: number }) {
  const [detail, setDetail] = useState<MessageDetail | null>(null)
  const [conflicts, setConflicts] = useState<ConflictRow[]>([])
  const [conflictsExpanded, setConflictsExpanded] = useState(false)
  const [rsvp, setRsvp] = useState<RsvpStatus>('idle')

  useEffect(() => {
    let cancelled = false
    const run = async () => {
      try {
        const d = await fetchJson<MessageDetail | null>(`/mail/messages/${messageUid}`)
        if (!cancelled) setDetail(d)
      } catch {
        // network / auth failure — silently leave card empty
      }
    }
    void run()
    return () => {
      cancelled = true
    }
  }, [messageUid])

  // When invite_payload arrives, query the conflict window.
  useEffect(() => {
    const payload = detail?.invite_payload
    if (!payload) return
    const startIso = pickIso(payload.dtstart)
    const endIso = pickIso(payload.dtend) ?? startIso
    if (!startIso || !endIso) return
    let cancelled = false
    const run = async () => {
      try {
        const params = new URLSearchParams({
          end: endIso.endsWith('Z') ? endIso : `${endIso}Z`,
          exclude_uid: payload.uid,
          start: startIso.endsWith('Z') ? startIso : `${startIso}Z`,
        })
        const rows = await fetchJson<ConflictRow[]>(`/calendar/conflicts?${params.toString()}`)
        if (!cancelled) setConflicts(rows)
      } catch {
        if (!cancelled) setConflicts([])
      }
    }
    void run()
    return () => {
      cancelled = true
    }
  }, [detail])

  const send = async (partstat: 'ACCEPTED' | 'DECLINED' | 'TENTATIVE') => {
    setRsvp('pending')
    try {
      await postJson(`/invites/${messageUid}/rsvp`, { partstat })
      setRsvp('sent')
    } catch (e) {
      setRsvp({ error: e instanceof Error ? e.message : 'failed' })
    }
  }

  const payload = detail?.invite_payload
  const badge = useMemo(() => (payload ? methodBadge(payload.method) : null), [payload])

  if (!payload || !badge) return null

  const range = formatLocalRange(payload.dtstart, payload.dtend)
  const cancelled = payload.method.toUpperCase() === 'CANCEL'

  return (
    <div className="border-bd bg-bg-elevated my-3 rounded-lg border p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-2">
          <Calendar className="text-fg-muted h-4 w-4" />
          <span className={`rounded px-2 py-0.5 text-xs font-medium ${badge.className}`}>
            {badge.label}
          </span>
        </div>
      </div>

      <div className="mt-2">
        <div className="text-fg text-base font-semibold">{payload.summary}</div>
        {range && (
          <div className="text-fg-muted mt-1 flex items-center gap-1 text-sm">
            <Clock className="h-3.5 w-3.5" />
            {range}
          </div>
        )}
        {payload.location && (
          <div className="text-fg-muted mt-1 flex items-center gap-1 text-sm">
            <MapPin className="h-3.5 w-3.5" />
            {payload.location}
          </div>
        )}
        {payload.organizer && (
          <div className="text-fg-muted mt-1 text-xs">
            Organizer:{' '}
            {payload.organizer.cn
              ? `${payload.organizer.cn} <${payload.organizer.email}>`
              : payload.organizer.email}
          </div>
        )}
        {payload.attendees && payload.attendees.length > 0 && (
          <div className="text-fg-muted mt-1 flex items-center gap-1 text-xs">
            <Users className="h-3 w-3" />
            {payload.attendees.length} attendee
            {payload.attendees.length === 1 ? '' : 's'}
          </div>
        )}
      </div>

      {conflicts.length > 0 && !cancelled && (
        <div className="mt-3 rounded border border-amber-500/30 bg-amber-500/5 p-2 text-xs">
          {conflicts.length === 1 ? (
            <div className="text-amber-300">
              ⚠ Conflicts with {conflicts[0].summary}
              {conflicts[0].dtstart && (
                <span className="text-fg-muted ml-1">({formatDateTime(conflicts[0].dtstart)})</span>
              )}
            </div>
          ) : (
            <div>
              <button
                className="text-amber-300 hover:underline"
                onClick={() => setConflictsExpanded((v) => !v)}
              >
                ⚠ Conflicts with {conflicts.length} events {conflictsExpanded ? '(hide)' : '(show)'}
              </button>
              {conflictsExpanded && (
                <ul className="text-fg-muted mt-1 space-y-0.5">
                  {conflicts.map((c) => (
                    <li key={c.uid}>
                      • {c.summary}
                      {c.dtstart && <span className="ml-1">({formatDateTime(c.dtstart)})</span>}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
      )}

      {!cancelled && (
        <div className="mt-3 flex items-center gap-2">
          <button
            className="border-bd bg-bg flex items-center gap-1 rounded border px-3 py-1.5 text-sm hover:bg-emerald-500/10 disabled:opacity-50"
            disabled={rsvp === 'pending' || rsvp === 'sent'}
            onClick={() => void send('ACCEPTED')}
          >
            <Check className="h-3.5 w-3.5" />
            Accept
          </button>
          <button
            className="border-bd bg-bg flex items-center gap-1 rounded border px-3 py-1.5 text-sm hover:bg-amber-500/10 disabled:opacity-50"
            disabled={rsvp === 'pending' || rsvp === 'sent'}
            onClick={() => void send('TENTATIVE')}
          >
            Tentative
          </button>
          <button
            className="border-bd bg-bg flex items-center gap-1 rounded border px-3 py-1.5 text-sm hover:bg-red-500/10 disabled:opacity-50"
            disabled={rsvp === 'pending' || rsvp === 'sent'}
            onClick={() => void send('DECLINED')}
          >
            <X className="h-3.5 w-3.5" />
            Decline
          </button>
          <span className="text-fg-muted ml-2 text-xs">
            {rsvp === 'pending' && 'sending…'}
            {rsvp === 'sent' && '✓ reply sent'}
            {typeof rsvp === 'object' && `error: ${rsvp.error}`}
          </span>
        </div>
      )}
    </div>
  )
}

function formatDateTime(dt: CalDateTime | null | undefined): string {
  const iso = pickIso(dt)
  if (!iso) return ''
  // Force local-tz formatting via the browser Intl API.
  const d = new Date(iso.endsWith('Z') ? iso : iso + 'Z')
  if (isNaN(d.getTime())) return iso
  return d.toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  })
}

function formatLocalRange(
  start: CalDateTime | null | undefined,
  end: CalDateTime | null | undefined
): string {
  const s = formatDateTime(start)
  const e = formatDateTime(end)
  if (!s) return ''
  if (!e) return s
  return `${s} → ${e}`
}

function methodBadge(method: string): { className: string; label: string } {
  switch (method.toUpperCase()) {
    case 'CANCEL':
      return { className: 'bg-red-500/15 text-red-300', label: 'Cancelled' }
    case 'COUNTER':
      return { className: 'bg-amber-500/15 text-amber-300', label: 'Counter-proposed' }
    case 'REPLY':
      return { className: 'bg-blue-500/15 text-blue-300', label: 'Reply' }
    case 'REQUEST':
      return { className: 'bg-emerald-500/15 text-emerald-300', label: 'New invite' }
    case 'UPDATE':
      return { className: 'bg-sky-500/15 text-sky-300', label: 'Updated' }
    default:
      return { className: 'bg-zinc-500/15 text-zinc-300', label: method }
  }
}

function pickIso(dt: CalDateTime | null | undefined): null | string {
  if (!dt) return null
  if (typeof dt === 'string') return dt
  return dt.iso ?? null
}
