// Turning an iCalendar date-time into something a person reads. Split out
// of invite-card on 2026-08-02; the component is JSX, this is not.

// Mirrors crates/server/src/ical/mod.rs CalDateTime — externally-tagged
// enum from Rust derive(Serialize). Real wire shapes:
//   { "Utc": "2026-05-01T14:00:00Z" }
//   { "Floating": "2026-05-01T14:00:00" }
//   { "Zoned": { "tz_name": "Asia/Tokyo", "local": "2026-05-01T14:00:00" } }
//   { "Date": "2026-05-01" }
export type CalDateTime =
  | string // tolerant fallback
  | { Date: string }
  | { Floating: string }
  | { Utc: string }
  | { Zoned: { local: string; tz_name: string } }

export function fmtHm(d: Date): string {
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}

/// Narrow timeline render: drop the year, and if start and end fall on the
/// same calendar day, render only the time of `end`. So a 60-min meeting
/// becomes `4月16日 19:30 → 20:00` instead of `2026年4月16日 19:30 →
/// 2026年4月16日 20:00`. Returns '' when there's no start to show.
export function formatCompactRange(
  start: CalDateTime | null | undefined,
  end: CalDateTime | null | undefined
): string {
  const sd = toLocalDate(start)
  if (!sd) return ''
  const sLabel = `${sd.getMonth() + 1}月${sd.getDate()}日 ${fmtHm(sd)}`
  const ed = toLocalDate(end)
  if (!ed) return sLabel
  const sameDay =
    sd.getFullYear() === ed.getFullYear() &&
    sd.getMonth() === ed.getMonth() &&
    sd.getDate() === ed.getDate()
  const eLabel = sameDay ? fmtHm(ed) : `${ed.getMonth() + 1}月${ed.getDate()}日 ${fmtHm(ed)}`
  return `${sLabel} → ${eLabel}`
}

export function formatDateTime(dt: CalDateTime | null | undefined): string {
  const iso = pickIso(dt)
  if (!iso) return ''
  const d = toLocalDate(dt)
  if (!d) return iso
  // A date is a date. An all-day event has no offset, and giving it one
  // is how it lands on the wrong day for readers west of the organiser.
  if (isDateOnly(dt)) {
    return d.toLocaleDateString(undefined, { dateStyle: 'medium' })
  }
  return d.toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  })
}

export function formatLocalRange(
  start: CalDateTime | null | undefined,
  end: CalDateTime | null | undefined
): string {
  const s = formatDateTime(start)
  const e = formatDateTime(end)
  if (!s) return ''
  if (!e) return s
  return `${s} → ${e}`
}

/// The event's own zone, when it is not the reader's.
///
/// Gmail shows both, and it is right to: "08:00" alone is a different
/// claim from "08:00 here, 16:00 where the organiser scheduled it", and
/// the second is what somebody joining a call across the Pacific needs
/// to check.
export function formatOrganiserTime(
  dt: CalDateTime | null | undefined,
  tzName: null | string | undefined
): null | string {
  if (!dt || !tzName) return null
  const iso = pickIso(dt)
  if (!iso || isDateOnly(dt)) return null
  const reader = Intl.DateTimeFormat().resolvedOptions().timeZone
  if (!reader || sameZone(reader, tzName)) return null
  // The wall-clock the organiser wrote, which is already in their zone —
  // no conversion, just a label. Converting it again through Intl would
  // require the browser to know a Windows zone name, which it does not.
  const wall = iso.replace(/Z$/, '')
  const hhmm = wall.slice(11, 16)
  return hhmm ? `${hhmm} ${tzName}` : null
}

export function isDateOnly(dt: CalDateTime | null | undefined): boolean {
  return !!dt && typeof dt !== 'string' && 'Date' in dt
}

export function isUtc(dt: CalDateTime | null | undefined): boolean {
  if (!dt) return false
  if (typeof dt === 'string') return dt.endsWith('Z')
  return 'Utc' in dt
}

export function pickIso(dt: CalDateTime | null | undefined): null | string {
  if (!dt) return null
  if (typeof dt === 'string') return dt
  if ('Utc' in dt) return dt.Utc
  if ('Floating' in dt) return dt.Floating
  if ('Zoned' in dt) return dt.Zoned.local
  if ('Date' in dt) return dt.Date
  return null
}

export function toLocalDate(dt: CalDateTime | null | undefined): Date | null {
  const iso = pickIso(dt)
  if (!iso) return null
  // `Zoned` and `Floating` carry a wall-clock with no offset. Reading
  // one as UTC — which this did until 2026-08-20 — put a 16:00 meeting
  // in Santa Clara at 01:00 the next morning in Tokyo. The instant now
  // arrives resolved from the server (`dtstart_utc`), computed against
  // the invitation's own VTIMEZONE; callers pass it here as a `Utc`.
  // What is left in this branch is a floating time, which means "local
  // to whoever is reading" — so reading it as local is correct.
  const parseable = isUtc(dt) ? iso : iso.replace(/Z$/, '')
  const d = new Date(parseable)
  return isNaN(d.getTime()) ? null : d
}

/// The zone the organiser wrote the time in, when the wire carries one.
export function zoneNameOf(dt: CalDateTime | null | undefined): null | string {
  if (!dt || typeof dt === 'string') return null
  return 'Zoned' in dt ? dt.Zoned.tz_name : null
}

/// Two zone names that mean the same place. Exact match, plus the
/// Windows spellings Exchange sends, which are the ones that turn up on
/// every Teams invitation.
function sameZone(reader: string, event: string): boolean {
  if (reader === event) return true
  const windowsToIana: Record<string, string> = {
    'Central Standard Time': 'America/Chicago',
    'China Standard Time': 'Asia/Shanghai',
    'Eastern Standard Time': 'America/New_York',
    'GMT Standard Time': 'Europe/London',
    'Pacific Standard Time': 'America/Los_Angeles',
    'Tokyo Standard Time': 'Asia/Tokyo',
    'W. Europe Standard Time': 'Europe/Berlin',
  }
  return windowsToIana[event] === reader
}
