import { Building2, Calendar, Car, ExternalLink, Plane, ShoppingBag, UtensilsCrossed } from 'lucide-react'
import type { StructuredData } from '@/lib/types'

export function StructuredDataCard({ data }: { data: StructuredData }) {
  const reservations = data.reservations ?? []
  const orders = data.orders ?? []
  const events = data.events ?? []
  const actions = data.actions ?? []

  const hasContent =
    reservations.length > 0 ||
    orders.length > 0 ||
    events.length > 0 ||
    actions.length > 0

  if (!hasContent) return null

  return (
    <div className="border-b border-zinc-200 px-5 py-3 dark:border-zinc-800">
      <p className="mb-2 text-[11px] font-medium uppercase tracking-wider text-zinc-400">
        Structured Data
      </p>
      <div className="space-y-2">
        {reservations.map((r, i) => (
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

        {orders.map((o, i) => (
          <div
            key={`ord-${i}`}
            className="rounded-xl border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/50"
          >
            <div className="flex items-center gap-2">
              <ShoppingBag className="h-4 w-4 text-emerald-500" />
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

        {events.map((e, i) => (
          <div
            key={`evt-${i}`}
            className="rounded-xl border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/50"
          >
            <div className="flex items-center gap-2">
              <Calendar className="h-4 w-4 text-blue-500" />
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

        {actions.map((a, i) => (
          <div key={`act-${i}`} className="rounded-xl border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/50">
            <div className="flex items-center gap-2">
              <ExternalLink className="h-4 w-4 text-blue-600" />
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
      return <Plane className="h-4 w-4 text-sky-500" />
    case 'hotel':
      return <Building2 className="h-4 w-4 text-amber-500" />
    case 'restaurant':
      return <UtensilsCrossed className="h-4 w-4 text-orange-500" />
    default:
      return <Car className="h-4 w-4 text-purple-500" />
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
