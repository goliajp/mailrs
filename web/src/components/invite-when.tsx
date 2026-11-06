import type { CalDateTime } from '@/lib/invite-time'
import type { InvitePayload } from '@/lib/invite-types'

import { Clock, Repeat } from 'lucide-react'

import { currentLocale, type Translate, useT } from '@/i18n'
import { recurrenceText, shownSpan } from '@/lib/invite-recurrence'
import {
  formatLocalRange,
  formatOrganiserClock,
  formatOrganiserTime,
  pickIso,
  zoneNameOf,
} from '@/lib/invite-time'

/// When the meeting is: in the reader's zone, with the organiser's own
/// clock beside it when the two differ, and for a recurring meeting how
/// it repeats and which occurrence is next.
export function InviteWhen({ payload }: { payload: InvitePayload }) {
  const t = useT()
  const { end, start } = shownSpan(payload)
  const range = formatLocalRange(start, end)
  const organiserTime = organiserLabel(payload, start)
  const rule = payload.recurrence
  const upcoming = !!payload.next_occurrence
  return (
    <>
      {range && (
        <div className="text-fg-muted mt-1 flex items-start gap-1 text-sm">
          <Clock className="mt-[3px] h-3.5 w-3.5 shrink-0" />
          {/* One run of text, so a narrow card wraps it like a sentence
              instead of stacking the label, the dates and the zone as
              separate pieces. */}
          <span className="min-w-0" data-testid="invite-next">
            {whenLine(upcoming, range, t)}
            {/* The organiser's own zone beside the reader's, when they
                differ. "08:00" alone is a different claim from "08:00
                here, 16:00 where it was scheduled", and the second is
                what somebody joining across an ocean checks. */}
            {organiserTime && <span className="text-fg-muted/70"> · {organiserTime}</span>}
          </span>
        </div>
      )}
      {rule && (
        <div
          className="text-fg-muted mt-1 flex items-start gap-1 text-sm"
          data-testid="invite-repeat"
        >
          <Repeat className="mt-[3px] h-3.5 w-3.5 shrink-0" />
          <span className="min-w-0">
            {repeatLine(
              recurrenceText(rule, t, currentLocale(), pickIso(payload.dtstart ?? null)),
              !!organiserTime,
              payload.next_occurrence === null,
              t
            )}
          </span>
        </div>
      )}
    </>
  )
}

/// The organiser's clock for the shown start. Formatted from the instant
/// in the organiser's IANA zone when the server named one, so the
/// abbreviation is the one in force that day; otherwise the wall-clock
/// the server read off the invitation, labelled with its `TZID`.
function organiserLabel(p: InvitePayload, start: CalDateTime | null | undefined): null | string {
  const zone = p.organiser_zone
  if (zone && start && typeof start !== 'string' && 'Utc' in start) {
    return formatOrganiserClock(start.Utc, zone)
  }
  const tzName = zoneNameOf(p.dtstart)
  const local = p.next_occurrence?.organiser_local
  if (local) return formatOrganiserTime({ Floating: local }, tzName)
  return formatOrganiserTime(p.dtstart, tzName)
}

/// The rule in words, then whose clock it runs on and whether it is over.
/// The translation places the organiser note: the space before
/// "(organizer's time)" is English typography and wrong before "（".
function repeatLine(words: string, organiserTime: boolean, ended: boolean, t: Translate): string {
  let line = words
  if (organiserTime) line = t('invite.repeat.organiserTime', { rule: line })
  if (ended) line = `${line} · ${t('invite.repeat.ended')}`
  return line
}

function whenLine(upcoming: boolean, range: string, t: Translate): string {
  if (upcoming) return t('invite.repeat.next', { when: range })
  return range
}
