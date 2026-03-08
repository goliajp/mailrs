import type { StructuredData } from '@/lib/types'

export function StructuredDataCard({ data }: { data: StructuredData }) {
  const hasContent =
    data.reservations.length > 0 ||
    data.orders.length > 0 ||
    data.events.length > 0 ||
    data.actions.length > 0

  if (!hasContent) return null

  return (
    <div className="border-b border-zinc-200 px-5 py-3 dark:border-zinc-800">
      <p className="mb-2 text-[11px] font-medium uppercase tracking-wider text-zinc-400">
        Structured Data
      </p>
      <div className="space-y-2">
        {data.reservations.map((r, i) => (
          <div
            key={`res-${i}`}
            className="rounded-xl border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/50"
          >
            <div className="flex items-center gap-2">
              <ReservationIcon kind={r.type} />
              <span className="text-xs font-medium capitalize text-zinc-700 dark:text-zinc-300">
                {r.type} Reservation
              </span>
              {r.reservation_id && (
                <span className="rounded bg-zinc-200 px-1.5 py-0.5 text-[11px] font-mono text-zinc-600 dark:bg-zinc-700 dark:text-zinc-400">
                  {r.reservation_id}
                </span>
              )}
            </div>
            <div className="mt-1.5 space-y-0.5 text-xs text-zinc-600 dark:text-zinc-400">
              {r.name && <p>{r.name}</p>}
              {r.provider && <p className="text-zinc-500">{r.provider}</p>}
              {r.departure_airport && r.arrival_airport && (
                <p className="font-medium">
                  {r.departure_airport} → {r.arrival_airport}
                  {r.flight_number && ` (${r.flight_number})`}
                </p>
              )}
              {r.start_date && (
                <p>
                  {formatSchemaDate(r.start_date)}
                  {r.end_date && ` — ${formatSchemaDate(r.end_date)}`}
                </p>
              )}
              {r.location && <p>{r.location}</p>}
            </div>
          </div>
        ))}

        {data.orders.map((o, i) => (
          <div
            key={`ord-${i}`}
            className="rounded-xl border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/50"
          >
            <div className="flex items-center gap-2">
              <svg className="h-4 w-4 text-emerald-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 10.5V6a3.75 3.75 0 10-7.5 0v4.5m11.356-1.993l1.263 12c.07.665-.45 1.243-1.119 1.243H4.25a1.125 1.125 0 01-1.12-1.243l1.264-12A1.125 1.125 0 015.513 7.5h12.974c.576 0 1.059.435 1.119 1.007zM8.625 10.5a.375.375 0 11-.75 0 .375.375 0 01.75 0zm7.5 0a.375.375 0 11-.75 0 .375.375 0 01.75 0z" />
              </svg>
              <span className="text-xs font-medium text-zinc-700 dark:text-zinc-300">
                Order
              </span>
              {o.order_number && (
                <span className="rounded bg-zinc-200 px-1.5 py-0.5 text-[11px] font-mono text-zinc-600 dark:bg-zinc-700 dark:text-zinc-400">
                  #{o.order_number}
                </span>
              )}
              {o.merchant && (
                <span className="text-xs text-zinc-500">{o.merchant}</span>
              )}
            </div>
            {o.items.length > 0 && (
              <ul className="mt-1.5 space-y-0.5 text-xs text-zinc-600 dark:text-zinc-400">
                {o.items.map((item, j) => (
                  <li key={j} className="flex items-center gap-2">
                    <span>{item.name}</span>
                    {item.quantity && item.quantity > 1 && (
                      <span className="text-zinc-400">x{item.quantity}</span>
                    )}
                    {item.price && (
                      <span className="text-zinc-500">{item.price}</span>
                    )}
                  </li>
                ))}
              </ul>
            )}
            {o.total && (
              <p className="mt-1.5 text-xs font-medium text-zinc-700 dark:text-zinc-300">
                Total: {o.currency && `${o.currency} `}{o.total}
              </p>
            )}
          </div>
        ))}

        {data.events.map((e, i) => (
          <div
            key={`evt-${i}`}
            className="rounded-xl border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/50"
          >
            <div className="flex items-center gap-2">
              <svg className="h-4 w-4 text-blue-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 3v2.25M17.25 3v2.25M3 18.75V7.5a2.25 2.25 0 012.25-2.25h13.5A2.25 2.25 0 0121 7.5v11.25m-18 0A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75m-18 0v-7.5A2.25 2.25 0 015.25 9h13.5A2.25 2.25 0 0121 11.25v7.5" />
              </svg>
              <span className="text-xs font-medium text-zinc-700 dark:text-zinc-300">
                {e.name}
              </span>
            </div>
            <div className="mt-1 space-y-0.5 text-xs text-zinc-600 dark:text-zinc-400">
              {e.start_date && (
                <p>
                  {formatSchemaDate(e.start_date)}
                  {e.end_date && ` — ${formatSchemaDate(e.end_date)}`}
                </p>
              )}
              {e.location && <p>{e.location}</p>}
              {e.url && isSafeUrl(e.url) && (
                <a
                  href={e.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-blue-500 hover:underline"
                >
                  Details
                </a>
              )}
            </div>
          </div>
        ))}

        {data.actions.map((a, i) => (
          <div key={`act-${i}`} className="rounded-xl border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/50">
            <div className="flex items-center gap-2">
              <svg className="h-4 w-4 text-red-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
              </svg>
              <span className="text-xs font-medium text-zinc-700 dark:text-zinc-300">
                {a.type || 'Action'}: {a.name}
              </span>
            </div>
            {a.url && isSafeUrl(a.url) && (
              <a href={a.url} target="_blank" rel="noopener noreferrer"
                className="mt-1 inline-block text-xs text-blue-500 hover:underline">
                {a.name || 'Open'}
              </a>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}

function ReservationIcon({ kind }: { kind: string }) {
  switch (kind) {
    case 'flight':
      return (
        <svg className="h-4 w-4 text-sky-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
          <path strokeLinecap="round" strokeLinejoin="round" d="M6 12L3.269 3.126A59.768 59.768 0 0121.485 12 59.77 59.77 0 013.27 20.876L5.999 12zm0 0h7.5" />
        </svg>
      )
    case 'hotel':
      return (
        <svg className="h-4 w-4 text-amber-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
          <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 21h19.5m-18-18v18m10.5-18v18m6-13.5V21M6.75 6.75h.75m-.75 3h.75m-.75 3h.75m3-6h.75m-.75 3h.75m-.75 3h.75M6.75 21v-3.375c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125V21M3 3h12m-.75 4.5H21m-3.75 3.75h.008v.008h-.008v-.008zm0 3h.008v.008h-.008v-.008zm0 3h.008v.008h-.008v-.008z" />
        </svg>
      )
    case 'restaurant':
      return (
        <svg className="h-4 w-4 text-orange-500" viewBox="0 0 24 24" fill="currentColor">
          <path d="M11 9H9V2H7v7H5V2H3v7c0 2.12 1.66 3.84 3.75 3.97V22h2.5v-9.03C11.34 12.84 13 11.12 13 9V2h-2v7zm5-3v8h2.5v8H21V2c-2.76 0-5 2.24-5 4z" />
        </svg>
      )
    default:
      return (
        <svg className="h-4 w-4 text-purple-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
          <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 18.75a1.5 1.5 0 01-3 0m3 0a1.5 1.5 0 00-3 0m3 0h6m-9 0H3.375a1.125 1.125 0 01-1.125-1.125V14.25m17.25 4.5a1.5 1.5 0 01-3 0m3 0a1.5 1.5 0 00-3 0m3 0h1.125c.621 0 1.129-.504 1.09-1.124a17.902 17.902 0 00-3.213-9.193 2.056 2.056 0 00-1.58-.86H14.25M16.5 18.75h-2.25m0-11.177v-.958c0-.568-.422-1.048-.987-1.106a48.554 48.554 0 00-10.026 0 1.106 1.106 0 00-.987 1.106v7.635m12-6.677v6.677m0 4.5v-4.5m0 0h-12" />
        </svg>
      )
  }
}

// only allow http/https URLs to prevent javascript: XSS from untrusted email JSON-LD
function isSafeUrl(url: string): boolean {
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

function formatSchemaDate(dateStr: string): string {
  try {
    const d = new Date(dateStr)
    if (isNaN(d.getTime())) return dateStr
    return d.toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  } catch {
    return dateStr
  }
}
