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
  // Utc carries trailing 'Z'; Floating / Zoned / Date are wall-clock only —
  // treat those as UTC for display (the resulting tz-shift in the user's
  // locale is acceptable for v1; precise zoned conversion lands when we
  // round-trip the tz_name through chrono on the server side).
  const parseable = isUtc(dt) ? iso : `${iso.replace(/Z$/, '')}Z`
  const d = new Date(parseable)
  if (isNaN(d.getTime())) return iso
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
  const parseable = isUtc(dt) ? iso : `${iso.replace(/Z$/, '')}Z`
  const d = new Date(parseable)
  return isNaN(d.getTime()) ? null : d
}
