import type { CalDateTime } from '@/lib/invite-time'
import type { Attendee } from '@/lib/invite-types'

import { useQuery } from '@tanstack/react-query'
import { Calendar, Check, Clock, MapPin, Users, Video, X } from 'lucide-react'
import { useMemo, useState } from 'react'

import { answerWanted, inviteBadge } from '@/lib/invite-badge'
import { guestSummary, partstatDot, partstatWord } from '@/lib/invite-guests'
import { joinLinkOf } from '@/lib/invite-join'
import {
  formatCompactRange,
  formatDateTime,
  formatEpochOrIso,
  formatLocalRange,
  formatOrganiserTime,
  pickIso,
  zoneNameOf,
} from '@/lib/invite-time'
import { queryClient } from '@/lib/query-client'
import { calendarKeys, messageKeys } from '@/lib/query-keys'
import { adminListGet, adminObjectGet, adminPost } from '@/wire/endpoints/admin'

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
  /// The instant, resolved on the server against the invitation's own
  /// VTIMEZONE. Absent for an all-day event, which has no instant, and
  /// for mail ingested before 2026-08-20.
  dtend_utc?: null | string
  dtstart?: CalDateTime
  dtstart_utc?: null | string
  /// The way into the meeting, resolved on the server. RFC 5545 has no
  /// field for it and producers put it wherever, so one implementation
  /// rather than three.
  join_url?: null | string
  location?: null | string
  method: string
  organizer?: null | Person
  recurrence_id?: CalDateTime | null
  rrule?: null | string
  sequence: number
  status?: null | string
  summary: string
  uid: string
}

type MessageDetail = {
  invite_method: null | string
  invite_payload: InvitePayload | null
  /// MRS-19: previously-recorded RSVP partstat
  /// (ACCEPTED / TENTATIVE / DECLINED). Null = user hasn't replied yet.
  rsvp_at?: null | string
  rsvp_status?: null | string
  uid: number
}

type Person = { cn: null | string; email: string }

type RsvpStatus = 'idle' | 'pending' | 'sent' | { error: string }

export function InviteCard({
  compact = false,
  messageUid,
}: {
  compact?: boolean
  messageUid: number
}) {
  const detailQuery = useQuery({
    queryKey: messageKeys.detail(messageUid),
    queryFn: async () => {
      try {
        return await adminObjectGet<MessageDetail | null>(`/mail/messages/${messageUid}`)
      } catch {
        // network / auth failure — silently leave card empty
        return null
      }
    },
  })
  const detail = detailQuery.data ?? null
  const payloadForConflicts = detail?.invite_payload
  const startIso = payloadForConflicts ? pickIso(payloadForConflicts.dtstart) : null
  const endIso = payloadForConflicts ? (pickIso(payloadForConflicts.dtend) ?? startIso) : null
  // Skip conflict lookup in compact mode — narrow timeline rows show one line,
  // no room for "⚠ Conflicts with N events"; the main panel still does it.
  const conflictsEnabled = !compact && !!(payloadForConflicts && startIso && endIso)
  const conflictStart = startIso ? (startIso.endsWith('Z') ? startIso : `${startIso}Z`) : ''
  const conflictEnd = endIso ? (endIso.endsWith('Z') ? endIso : `${endIso}Z`) : ''
  const conflictExcludeUid = payloadForConflicts?.uid ?? ''
  const conflictsQuery = useQuery({
    enabled: conflictsEnabled,
    queryKey: calendarKeys.conflicts(conflictStart, conflictEnd, conflictExcludeUid),
    queryFn: async () => {
      try {
        const params = new URLSearchParams({
          end: conflictEnd,
          exclude_uid: conflictExcludeUid,
          start: conflictStart,
        })
        return await adminListGet<ConflictRow>(`/calendar/conflicts?${params.toString()}`)
      } catch {
        return [] as ConflictRow[]
      }
    },
  })
  const conflicts = conflictsQuery.data ?? []
  const [conflictsExpanded, setConflictsExpanded] = useState(false)
  const [rsvp, setRsvp] = useState<RsvpStatus>('idle')
  const [counterOpen, setCounterOpen] = useState(false)
  const [counterStart, setCounterStart] = useState('')
  const [counterEnd, setCounterEnd] = useState('')
  // Allow the user to change their RSVP after the persisted status renders.
  const [overridePersisted, setOverridePersisted] = useState(false)

  const send = async (partstat: 'ACCEPTED' | 'DECLINED' | 'TENTATIVE') => {
    setRsvp('pending')
    try {
      const body: { partstat: string; recurrence_id?: string } = { partstat }
      if (recurrenceIso) {
        body.recurrence_id = recurrenceIso.endsWith('Z') ? recurrenceIso : `${recurrenceIso}Z`
      }
      // The server returns 200 with { success: false } when it can't find
      // the invite (route mismatch, RBAC, race). HTTP-level success alone
      // is not enough — fail loud here so the UI doesn't lie about the
      // reply going out (MRS-20 incident).
      const res = await adminPost<{ message?: string; success: boolean }>(
        `/invites/${messageUid}/rsvp`,
        body
      )
      if (!res.success) {
        setRsvp({ error: res.message ?? 'reply rejected by server' })
        return
      }
      setRsvp('sent')
      // Reload detail so the persisted rsvp_status pill shows up
      // immediately rather than on next refresh.
      void queryClient.invalidateQueries({ queryKey: messageKeys.detail(messageUid) })
    } catch (e) {
      setRsvp({ error: e instanceof Error ? e.message : 'failed' })
    }
  }

  const sendCounter = async () => {
    if (!counterStart) return
    setRsvp('pending')
    try {
      // <input type="datetime-local"> yields a wall-clock string with no
      // timezone — Date treats it as local time and toISOString() emits
      // the corresponding UTC. That's what the server expects.
      const startIso = new Date(counterStart).toISOString()
      const endIso = counterEnd ? new Date(counterEnd).toISOString() : null
      const body: { dtend?: null | string; dtstart: string } = { dtstart: startIso }
      if (endIso) body.dtend = endIso
      const res = await adminPost<{ message?: string; success: boolean }>(
        `/invites/${messageUid}/counter`,
        body
      )
      if (!res.success) {
        setRsvp({ error: res.message ?? 'counter rejected by server' })
        return
      }
      setRsvp('sent')
      setCounterOpen(false)
    } catch (e) {
      setRsvp({ error: e instanceof Error ? e.message : 'failed' })
    }
  }

  const payload = detail?.invite_payload
  const badge = useMemo(
    () => (payload ? inviteBadge(payload.method, payload.sequence) : null),
    [payload]
  )

  if (!payload || !badge) return null

  // The resolved instant when the server has one, the wall-clock only
  // when it does not (all-day, floating, or mail ingested before the
  // resolution existed). Reading a zoned wall-clock as UTC is what put
  // a 16:00 Santa Clara meeting at 01:00 in Tokyo.
  const startAt: CalDateTime | null | undefined = payload.dtstart_utc
    ? { Utc: payload.dtstart_utc }
    : payload.dtstart
  const endAt: CalDateTime | null | undefined = payload.dtend_utc
    ? { Utc: payload.dtend_utc }
    : payload.dtend
  const organiserTime = formatOrganiserTime(payload.dtstart, zoneNameOf(payload.dtstart))
  // From the server, which resolved it once for all three clients —
  // the local rule stays only as a fallback for events stored before
  // the field existed.
  const joinUrl = payload.join_url ?? joinLinkOf(payload.location, payload.description)
  const range = formatLocalRange(startAt, endAt)
  const cancelled = payload.method.toUpperCase() === 'CANCEL'
  // When the invite carries a RECURRENCE-ID, the organizer is targeting one
  // specific occurrence of a recurring series — RSVP applies to that
  // occurrence only (RFC 5545 §3.8.4.4 / RFC 5546 §3.4).
  const isSingleOccurrence = !!payload.recurrence_id
  const recurrenceIso = isSingleOccurrence ? pickIso(payload.recurrence_id ?? null) : null
  // MRS-19: server-persisted partstat ("ACCEPTED" / "TENTATIVE" / "DECLINED")
  // survives page refresh. Show the recorded state instead of fresh
  // buttons unless the user explicitly clicks Change.
  const persistedStatus = detail?.rsvp_status?.toUpperCase() ?? null
  const showPersisted = persistedStatus && !overridePersisted && rsvp !== 'sent'

  if (compact) {
    const statusLabel = persistedStatus ? compactStatusLabel(persistedStatus) : null
    // formatLocalRange ('2026年4月16日 19:30 → 2026年4月16日 20:00') overflowed
    // the narrow timeline card. compactRange drops year + same-day duplicate
    // and falls back to truncate via `min-w-0 truncate` if it still doesn't
    // fit a particularly narrow viewport.
    const compactR = formatCompactRange(startAt, endAt)
    return (
      <div className="border-border text-fg-muted my-1.5 flex min-w-0 items-center gap-1.5 rounded-md border px-2 py-1 text-xs">
        <Calendar className="h-3 w-3 shrink-0" />
        <span className="text-fg min-w-0 truncate font-medium">{payload.summary}</span>
        {compactR && <span className="min-w-0 shrink truncate opacity-70">· {compactR}</span>}
        {statusLabel && (
          <span className={`text-tiny shrink-0 rounded px-1.5 py-0.5 ${statusLabel.className}`}>
            {statusLabel.label}
          </span>
        )}
      </div>
    )
  }

  return (
    <div className="border-border bg-bg-secondary my-3 rounded-lg border p-4">
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
        {isSingleOccurrence && (
          <div className="mt-1 text-xs text-amber-300">
            ⓘ This occurrence of a recurring event — RSVP applies only to this instance.
          </div>
        )}
        {range && (
          <div className="text-fg-muted mt-1 flex items-center gap-1 text-sm">
            <Clock className="h-3.5 w-3.5" />
            {range}
            {/* The organiser's own zone beside the reader's, when they
                differ. "08:00" alone is a different claim from "08:00
                here, 16:00 where it was scheduled", and the second is
                what somebody joining across an ocean checks. */}
            {organiserTime && <span className="text-fg-muted/70">· {organiserTime}</span>}
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
          <details className="text-fg-muted mt-1 text-xs">
            {/* A count answers "how many"; the states answer "is this
                happening", which is the question somebody deciding
                whether to go actually has. Gmail shows both, and the
                summary line keeps the count for a glance. */}
            <summary className="flex cursor-pointer items-center gap-1">
              <Users className="h-3 w-3" />
              {guestSummary(payload.attendees)}
            </summary>
            <ul className="mt-1 ml-4 space-y-0.5">
              {payload.attendees.map((a) => (
                <li className="flex items-center gap-1.5" key={a.email}>
                  <span className={partstatDot(a.partstat)}>●</span>
                  <span className="truncate">{a.cn ?? a.email}</span>
                  <span className="opacity-60">{partstatWord(a.partstat)}</span>
                </li>
              ))}
            </ul>
          </details>
        )}
        {joinUrl && !cancelled && (
          <a
            className="text-accent mt-2 inline-flex items-center gap-1 text-xs hover:underline"
            href={joinUrl}
            rel="noreferrer noopener"
            target="_blank"
          >
            <Video className="h-3 w-3" />
            Join the meeting
          </a>
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
                aria-expanded={conflictsExpanded}
                className="text-amber-300 hover:underline"
                onClick={() => setConflictsExpanded((v) => !v)}
                type="button"
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

      {answerWanted(payload.method) && !cancelled && showPersisted && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <RsvpStatusPill at={detail?.rsvp_at ?? null} partstat={persistedStatus!} />
          <button
            className="text-fg-muted hover:text-fg ml-auto text-xs underline-offset-2 hover:underline"
            onClick={() => setOverridePersisted(true)}
            type="button"
          >
            Change
          </button>
        </div>
      )}

      {answerWanted(payload.method) && !cancelled && !showPersisted && (
        <>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              className="border-border text-fg hover:bg-bg-tertiary flex items-center gap-1 rounded-md border px-3 py-1.5 text-sm transition-colors disabled:opacity-50"
              disabled={rsvp === 'pending' || rsvp === 'sent'}
              onClick={() => void send('ACCEPTED')}
              type="button"
            >
              <Check className="h-3.5 w-3.5" />
              Accept
            </button>
            <button
              className="border-border text-fg hover:bg-bg-tertiary flex items-center gap-1 rounded-md border px-3 py-1.5 text-sm transition-colors disabled:opacity-50"
              disabled={rsvp === 'pending' || rsvp === 'sent'}
              onClick={() => void send('TENTATIVE')}
              type="button"
            >
              Tentative
            </button>
            <button
              className="border-border text-fg hover:bg-bg-tertiary flex items-center gap-1 rounded-md border px-3 py-1.5 text-sm transition-colors disabled:opacity-50"
              disabled={rsvp === 'pending' || rsvp === 'sent'}
              onClick={() => void send('DECLINED')}
              type="button"
            >
              <X className="h-3.5 w-3.5" />
              Decline
            </button>
            <button
              aria-expanded={counterOpen}
              className="text-fg-muted hover:text-fg ml-auto text-xs underline-offset-2 hover:underline disabled:opacity-50"
              disabled={rsvp === 'pending' || rsvp === 'sent'}
              onClick={() => setCounterOpen((v) => !v)}
              type="button"
            >
              {counterOpen ? 'Cancel counter-proposal' : 'Propose new time'}
            </button>
          </div>
          {rsvp !== 'idle' && (
            <div className="text-fg-muted mt-1.5 text-xs">
              {rsvp === 'pending' && 'sending…'}
              {rsvp === 'sent' && <span className="text-emerald-400">✓ reply sent</span>}
              {typeof rsvp === 'object' && (
                <span className="text-red-400">error: {rsvp.error}</span>
              )}
            </div>
          )}

          {counterOpen && (
            <div className="border-border mt-3 rounded border p-3">
              <div className="text-fg-muted mb-2 text-xs">
                Counter-proposal — your local time. Sends a METHOD=COUNTER reply to the organizer;
                their calendar surfaces it for accept/decline.
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <label className="text-fg-muted text-xs" htmlFor="counter-start">
                  Start
                </label>
                <input
                  aria-label="Counter-proposal start time"
                  className="border-border text-fg bg-bg-secondary rounded border px-2 py-1 text-sm"
                  id="counter-start"
                  onChange={(e) => setCounterStart(e.target.value)}
                  type="datetime-local"
                  value={counterStart}
                />
                <label className="text-fg-muted text-xs" htmlFor="counter-end">
                  End
                </label>
                <input
                  aria-label="Counter-proposal end time"
                  className="border-border text-fg bg-bg-secondary rounded border px-2 py-1 text-sm"
                  id="counter-end"
                  onChange={(e) => setCounterEnd(e.target.value)}
                  type="datetime-local"
                  value={counterEnd}
                />
                <button
                  className="bg-accent text-accent-fg hover:bg-accent-hover rounded-md px-3 py-1.5 text-sm transition-colors disabled:opacity-50"
                  disabled={!counterStart || rsvp === 'pending'}
                  onClick={() => void sendCounter()}
                  type="button"
                >
                  Send counter
                </button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  )
}

function compactStatusLabel(partstat: string): null | { className: string; label: string } {
  switch (partstat) {
    case 'ACCEPTED':
      return { className: 'bg-emerald-500/15 text-emerald-300', label: 'Accepted' }
    case 'DECLINED':
      return { className: 'bg-red-500/15 text-red-300', label: 'Declined' }
    case 'TENTATIVE':
      return { className: 'bg-amber-500/15 text-amber-300', label: 'Tentative' }
    default:
      return null
  }
}

/// "4 guests · 1 yes, 2 awaiting" — the shape Gmail uses, because the
/// count alone does not say whether the meeting is happening.
function RsvpStatusPill({ at, partstat }: { at: null | string; partstat: string }) {
  const label =
    partstat === 'ACCEPTED'
      ? 'You accepted'
      : partstat === 'TENTATIVE'
        ? 'You marked tentative'
        : partstat === 'DECLINED'
          ? 'You declined'
          : `You replied: ${partstat}`
  const cls =
    partstat === 'ACCEPTED'
      ? 'bg-emerald-500/15 text-emerald-300'
      : partstat === 'DECLINED'
        ? 'bg-red-500/15 text-red-300'
        : 'bg-amber-500/15 text-amber-300'
  const when = formatEpochOrIso(at)
  return (
    <span className={`flex items-center gap-1 rounded px-2.5 py-1 text-xs font-medium ${cls}`}>
      {label}
      {when && <span className="text-fg-muted ml-1 font-normal">{when}</span>}
    </span>
  )
}
