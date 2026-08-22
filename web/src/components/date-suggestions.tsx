import { useQuery } from '@tanstack/react-query'
import { CalendarPlus } from 'lucide-react'

import { chipLabel } from '@/lib/date-chip'
import { messageKeys } from '@/lib/query-keys'
import { adminObjectGet } from '@/wire/endpoints/admin'

/// A date somebody wrote in the body, offered as an event.
///
/// Most mail about a meeting is not an invitation: it carries no
/// calendar part, no UID, nothing to accept — just a sentence with a
/// time in it. Apple Mail has offered to turn those into events since
/// 2007, and a client without it makes the reader retype what is
/// already on the screen.
///
/// **It offers; it does not file.** The server proposes candidates and
/// nothing happens until this is clicked, because a guess about a date
/// is a guess.
export type DateSuggestion = {
  /// `YYYY-MM-DD`.
  date: string
  /// Wall-clock `YYYY-MM-DDTHH:MM:SS`, or null when only a day was
  /// written. Deliberately not an instant: the writer meant their own
  /// hour, and neither side knows which zone that was, so it renders
  /// as local — the same reading the person would make.
  datetime: null | string
  /// What they actually wrote, quoted back rather than reformatted.
  text: string
}

export function DateSuggestions({ suggestions }: { suggestions: DateSuggestion[] }) {
  if (suggestions.length === 0) return null
  return (
    <div className="flex flex-wrap items-center gap-2 px-4 py-2">
      <span className="text-fg-muted text-xs">Add to calendar:</span>
      {suggestions.map((s) => (
        <a
          className="border-border text-fg-muted hover:text-fg inline-flex items-center gap-1 rounded border px-2 py-0.5 text-xs"
          download={`${s.date}.ics`}
          href={icsHref(s)}
          key={`${s.date}-${s.datetime ?? 'allday'}`}
          title={s.text}
        >
          <CalendarPlus className="h-3 w-3" />
          {chipLabel(s)}
        </a>
      ))}
    </div>
  )
}

/// The offers for one message.
///
/// Shares `InviteCard`'s query key, so opening a message that has both
/// an invitation and prose dates costs one request rather than two.
export function DateSuggestionsForMessage({ messageUid }: { messageUid: number }) {
  const detail = useQuery({
    queryKey: messageKeys.detail(messageUid),
    queryFn: async () => {
      try {
        return await adminObjectGet<null | { date_suggestions?: DateSuggestion[] }>(
          `/mail/messages/${messageUid}`
        )
      } catch {
        return null
      }
    },
  })
  return <DateSuggestions suggestions={detail.data?.date_suggestions ?? []} />
}

function escapeText(v: string): string {
  return v.replace(/\\/g, '\\\\').replace(/;/g, '\\;').replace(/,/g, '\\,').replace(/\n/g, '\\n')
}

function hash(v: string): number {
  let h = 0
  for (const ch of v) h = (h * 31 + ch.codePointAt(0)!) | 0
  return h
}

/// The event as a downloadable `.ics`, built in the browser.
///
/// A file rather than a server round-trip because the reader may want
/// it in a calendar this server has never heard of, and because an
/// offer that files into our own store and nowhere else is an offer
/// most people cannot use.
///
/// The time is written **floating** — no zone, no `Z`. RFC 5545 §3.3.5
/// says a floating time means local to whoever reads it, which is
/// exactly what "2pm" in a sentence means and exactly what we know.
/// Stamping it UTC would move the appointment by the reader's offset.
function icsHref(s: DateSuggestion): string {
  const stamp = (iso: string) => iso.replace(/[-:]/g, '')
  const lines = ['BEGIN:VCALENDAR', 'VERSION:2.0', 'PRODID:-//mailrs//datefind//EN', 'BEGIN:VEVENT']
  lines.push(`UID:${s.date}-${Math.abs(hash(s.text))}@mailrs`)
  lines.push(`SUMMARY:${escapeText(s.text)}`)
  if (s.datetime) {
    const start = stamp(s.datetime)
    lines.push(`DTSTART:${start}`)
  } else {
    lines.push(`DTSTART;VALUE=DATE:${s.date.replace(/-/g, '')}`)
  }
  lines.push('END:VEVENT', 'END:VCALENDAR')
  return `data:text/calendar;charset=utf-8,${encodeURIComponent(lines.join('\r\n'))}`
}
