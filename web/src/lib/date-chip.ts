/// One shape for every date chip, whatever the writer typed.
///
/// A candidate quotes the source text back, which is honest and is what
/// an inline underline would show. Out of context in a row it reads as
/// noise: `Aug 21 2026` beside `2026-08-20` beside `2026-08-21` looks
/// like three unrelated things rather than three days. The written form
/// stays as the chip's `title` and as the event's summary, so nothing
/// is lost — only the row is made readable.
///
/// Built from the parts rather than `new Date(iso)`, because a bare
/// `YYYY-MM-DD` parses as UTC midnight and renders as the day before
/// for anybody west of it.
export function chipLabel(s: { date: string; datetime: null | string }): string {
  const [y, m, d] = s.date.split('-').map(Number)
  const date = new Date(y, m - 1, d).toLocaleDateString(undefined, {
    day: 'numeric',
    month: 'short',
    weekday: 'short',
  })
  if (!s.datetime) return date
  // `Number('')` is 0, not NaN, so an empty clock would render as
  // midnight — a date with no hour would silently gain one.
  const clock = s.datetime.split('T')[1] ?? ''
  if (!/^\d{1,2}:\d{2}/.test(clock)) return date
  const [hh, mm] = clock.split(':').map(Number)
  const at = new Date(y, m - 1, d, hh, mm || 0)
  return `${date}, ${at.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })}`
}
