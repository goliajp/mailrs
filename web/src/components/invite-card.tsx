import type { InvitePayload, Person } from '@/lib/invite-types'
import type { RsvpBody } from '@/wire/endpoints/invites'

import { useQuery } from '@tanstack/react-query'
import { Calendar, Check, MapPin, Users, Video, X } from 'lucide-react'
import { useMemo, useState } from 'react'

import { type Key, useT } from '@/i18n'
import { answerWanted, inviteBadge } from '@/lib/invite-badge'
import { compactStatusLabel, guestSummary, partstatDot, partstatWord } from '@/lib/invite-guests'
import { joinLinkOf } from '@/lib/invite-join'
import { shownSpan } from '@/lib/invite-recurrence'
import { formatCompactRange, formatDateTime, pickIso } from '@/lib/invite-time'
import { queryClient } from '@/lib/query-client'
import { calendarKeys, messageKeys } from '@/lib/query-keys'
import { adminListGet, adminObjectGet } from '@/wire/endpoints/admin'
import { wireInviteCounter, wireInviteRsvp } from '@/wire/endpoints/invites'

import { RsvpStatusPill } from './invite-rsvp-pill'
import { InviteWhen } from './invite-when'

type ConflictRow = {
  dtend: null | string
  dtstart: null | string
  organizer: null | string
  status: null | string
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

type RsvpStatus = 'idle' | 'pending' | 'sent' | { error: string }

export function InviteCard({
  compact = false,
  messageUid,
}: {
  compact?: boolean
  messageUid: number
}) {
  const t = useT()
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
  // the occurrence the card shows — the next one of a recurring meeting —
  // as instants; an all-day or floating event has none and blocks nothing
  const shown = payloadForConflicts ? shownSpan(payloadForConflicts) : null
  const conflictStart = utcOf(shown?.start)
  const conflictEnd = utcOf(shown?.end) || conflictStart
  // Skip conflict lookup in compact mode — narrow timeline rows show one line,
  // no room for "⚠ Conflicts with N events"; the main panel still does it.
  const conflictsEnabled = !compact && !!conflictStart
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
      const body: RsvpBody = { partstat }
      if (recurrenceIso) {
        body.recurrence_id = recurrenceIso.endsWith('Z') ? recurrenceIso : `${recurrenceIso}Z`
      }
      // The server returns 200 with { success: false } when it can't find
      // the invite (route mismatch, RBAC, race). HTTP-level success alone
      // is not enough — fail loud here so the UI doesn't lie about the
      // reply going out (MRS-20 incident).
      const res = await wireInviteRsvp(messageUid, body)
      if (!res.success) {
        setRsvp({ error: res.message ?? 'reply rejected by server' })
        return
      }
      setRsvp('sent')
      // the persisted rsvp_status pill and the calendar event both changed
      void queryClient.invalidateQueries({ queryKey: messageKeys.detail(messageUid) })
      void queryClient.invalidateQueries({ queryKey: calendarKeys.all() })
    } catch (e) {
      setRsvp({ error: e instanceof Error ? e.message : 'failed' })
    }
  }

  const sendCounter = async () => {
    if (!counterStart || !counterEnd) return
    setRsvp('pending')
    try {
      // <input type="datetime-local"> yields a wall-clock string with no
      // timezone — Date treats it as local time and toISOString() emits
      // the corresponding UTC. That's what the server expects, under the
      // names its CounterRequest reads: `start` and `end`, both required
      const body = {
        end: new Date(counterEnd).toISOString(),
        start: new Date(counterStart).toISOString(),
      }
      const res = await wireInviteCounter(messageUid, body)
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

  // From the server, which resolved it once for all three clients —
  // the local rule stays only as a fallback for events stored before
  // the field existed.
  const joinUrl = payload.join_url ?? joinLinkOf(payload.location, payload.description)
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
    const compactR = formatCompactRange(shownSpan(payload).start, shownSpan(payload).end)
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
    // No frame of its own. The pane it renders into is already an
    // enclosed region, so a border and rounded corners here drew the
    // same box twice — a card inside a card. It fills the region edge
    // to edge and keeps `p-4`, which is what held the spacing inside
    // it; only the chrome is gone.
    <div className="bg-bg-secondary p-4">
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
          <div className="mt-1 text-xs text-amber-300">ⓘ {t('invite.singleOccurrence')}</div>
        )}
        <InviteWhen payload={payload} />
        {payload.location && (
          <div className="text-fg-muted mt-1 flex items-center gap-1 text-sm">
            <MapPin className="h-3.5 w-3.5" />
            {payload.location}
          </div>
        )}
        {payload.organizer && (
          <div className="text-fg-muted mt-1 text-xs">
            {t('invite.organizer', { name: personLine(payload.organizer) })}
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
            /* The most-used thing on a meeting invitation, and on a
               phone it was a line of 12px text between two other lines
               of 12px text. A filled target at 44px on small screens,
               back to a quiet link on a desktop where the pointer is
               precise and the row is not the only thing on screen. */
            className="bg-accent text-accent-fg sm:text-accent mt-3 flex min-h-11 w-full items-center justify-center gap-1.5 rounded-md text-sm font-medium sm:mt-2 sm:inline-flex sm:min-h-0 sm:w-auto sm:justify-start sm:bg-transparent sm:text-xs sm:font-normal sm:hover:underline"
            href={joinUrl}
            rel="noreferrer noopener"
            target="_blank"
          >
            <Video className="h-4 w-4 sm:h-3 sm:w-3" />
            {t('invite.join')}
          </a>
        )}
      </div>

      {conflicts.length > 0 && !cancelled && (
        <div className="mt-3 rounded border border-amber-500/30 bg-amber-500/5 p-2 text-xs">
          {conflicts.length === 1 ? (
            <div className="text-amber-300">
              ⚠ {t('invite.conflictsWith', { what: conflicts[0].summary })}
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
                ⚠ {t('invite.conflictsMany', { n: conflicts.length })}{' '}
                {t(showHide(conflictsExpanded))}
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
            {t('invite.change')}
          </button>
        </div>
      )}

      {answerWanted(payload.method) && !cancelled && !showPersisted && (
        <>
          {/* Equal thirds on a phone, natural width on a desktop. Three
              buttons that wrap onto two lines put "Decline" alone under
              the other two, which is where a thumb goes by accident.
              `min-h-11` is 44px — the smallest target Apple and Google
              both agree on, and these were 32. */}
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              className="border-border text-fg hover:bg-bg-tertiary flex min-h-11 flex-1 items-center justify-center gap-1 rounded-md border px-3 py-1.5 text-sm transition-colors disabled:opacity-50 sm:min-h-0 sm:flex-none"
              disabled={rsvp === 'pending' || rsvp === 'sent'}
              onClick={() => void send('ACCEPTED')}
              type="button"
            >
              <Check className="h-3.5 w-3.5" />
              {t('invite.accept')}
            </button>
            <button
              className="border-border text-fg hover:bg-bg-tertiary flex min-h-11 flex-1 items-center justify-center gap-1 rounded-md border px-3 py-1.5 text-sm transition-colors disabled:opacity-50 sm:min-h-0 sm:flex-none"
              disabled={rsvp === 'pending' || rsvp === 'sent'}
              onClick={() => void send('TENTATIVE')}
              type="button"
            >
              {t('invite.tentative')}
            </button>
            <button
              className="border-border text-fg hover:bg-bg-tertiary flex min-h-11 flex-1 items-center justify-center gap-1 rounded-md border px-3 py-1.5 text-sm transition-colors disabled:opacity-50 sm:min-h-0 sm:flex-none"
              disabled={rsvp === 'pending' || rsvp === 'sent'}
              onClick={() => void send('DECLINED')}
              type="button"
            >
              <X className="h-3.5 w-3.5" />
              {t('invite.decline')}
            </button>
            <button
              aria-expanded={counterOpen}
              className="text-fg-muted hover:text-fg ml-auto text-xs underline-offset-2 hover:underline disabled:opacity-50"
              disabled={rsvp === 'pending' || rsvp === 'sent'}
              onClick={() => setCounterOpen((v) => !v)}
              type="button"
            >
              {t(counterLabel(counterOpen))}
            </button>
          </div>
          {rsvp !== 'idle' && (
            <div className="text-fg-muted mt-1.5 text-xs">
              {rsvp === 'pending' && t('invite.sending')}
              {rsvp === 'sent' && <span className="text-emerald-400">✓ {t('invite.sent')}</span>}
              {typeof rsvp === 'object' && (
                <span className="text-red-400">{t('invite.error', { error: rsvp.error })}</span>
              )}
            </div>
          )}

          {counterOpen && (
            <div className="border-border mt-3 rounded border p-3">
              <div className="text-fg-muted mb-2 text-xs">{t('invite.counterNote')}</div>
              <div className="flex flex-wrap items-center gap-2">
                <label className="text-fg-muted text-xs" htmlFor="counter-start">
                  {t('invite.start')}
                </label>
                <input
                  aria-label={t('invite.counterStart')}
                  className="border-border text-fg bg-bg-secondary rounded border px-2 py-1 text-sm"
                  id="counter-start"
                  onChange={(e) => setCounterStart(e.target.value)}
                  type="datetime-local"
                  value={counterStart}
                />
                <label className="text-fg-muted text-xs" htmlFor="counter-end">
                  {t('invite.end')}
                </label>
                <input
                  aria-label={t('invite.counterEnd')}
                  className="border-border text-fg bg-bg-secondary rounded border px-2 py-1 text-sm"
                  id="counter-end"
                  onChange={(e) => setCounterEnd(e.target.value)}
                  type="datetime-local"
                  value={counterEnd}
                />
                <button
                  className="bg-accent text-accent-fg hover:bg-accent-hover rounded-md px-3 py-1.5 text-sm transition-colors disabled:opacity-50"
                  disabled={!counterStart || !counterEnd || rsvp === 'pending'}
                  onClick={() => void sendCounter()}
                  type="button"
                >
                  {t('invite.sendCounter')}
                </button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  )
}

function counterLabel(open: boolean): Key {
  if (open) return 'invite.counterCancel'
  return 'invite.counterPropose'
}

/** A person as the card names them: `Name <address>`, or the address alone. */
function personLine(person: Person): string {
  if (person.cn) return `${person.cn} <${person.email}>`
  return person.email
}

function showHide(expanded: boolean): Key {
  if (expanded) return 'common.hideParen'
  return 'common.showParen'
}

/// A shown time as the instant the conflicts query takes; '' for one with
/// no instant.
function utcOf(at: ReturnType<typeof shownSpan>['start'] | undefined): string {
  if (!at || typeof at === 'string' || !('Utc' in at)) return ''
  return at.Utc
}
