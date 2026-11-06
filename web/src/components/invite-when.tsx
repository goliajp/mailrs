import type { CalDateTime } from '@/lib/invite-time'
import type { InvitePayload } from '@/lib/invite-types'

import { Clock, Repeat } from 'lucide-react'

import { currentLocale, useT } from '@/i18n'
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
        <div className="text-fg-muted mt-1 flex flex-wrap items-center gap-1 text-sm">
          <Clock className="h-3.5 w-3.5" />
          {upcoming && <span>{t('invite.repeat.next')}</span>}
          {range}
          {/* The organiser's own zone beside the reader's, when they
              differ. "08:00" alone is a different claim from "08:00
              here, 16:00 where it was scheduled", and the second is
              what somebody joining across an ocean checks. */}
          {organiserTime && <span className="text-fg-muted/70">· {organiserTime}</span>}
        </div>
      )}
      {rule && (
        <div
          className="text-fg-muted mt-1 flex items-center gap-1 text-sm"
          data-testid="invite-repeat"
        >
          <Repeat className="h-3.5 w-3.5" />
          {recurrenceText(rule, t, currentLocale(), pickIso(payload.dtstart ?? null))}
          {organiserTime && <span>{t('invite.repeat.organiserTime')}</span>}
          {payload.next_occurrence === null && <span>· {t('invite.repeat.ended')}</span>}
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
