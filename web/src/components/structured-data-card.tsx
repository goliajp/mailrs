import type { StructuredData } from '@/lib/types'

import {
  Building2,
  Calendar,
  Car,
  ExternalLink,
  Plane,
  ShoppingBag,
  UtensilsCrossed,
} from 'lucide-react'

import { Copyable } from '@/components/copy-button'

export function StructuredDataCard({ data }: { data: StructuredData }) {
  const reservations = data.reservations ?? []
  const orders = data.orders ?? []
  const events = data.events ?? []
  const actions = data.actions ?? []

  const hasContent =
    reservations.length > 0 || orders.length > 0 || events.length > 0 || actions.length > 0

  if (!hasContent) return null

  return (
    <div className="border-border border-b px-5 py-3">
      <p className="text-fg-muted mb-2 text-xs font-medium tracking-wider uppercase">
        Structured Data
      </p>
      <div className="space-y-2">
        {reservations.map((r, i) => (
          <div className="border-border bg-bg-secondary rounded-lg border p-4" key={`res-${i}`}>
            <div className="flex items-center gap-2">
              <ReservationIcon kind={r.type} />
              <span className="text-fg-secondary text-xs font-medium capitalize">
                {r.type} Reservation
              </span>
              {r.reservation_id && (
                <Copyable value={r.reservation_id}>
                  <span className="bg-border text-fg-secondary md:text-mini rounded px-1.5 py-0.5 font-mono text-xs">
                    {r.reservation_id}
                  </span>
                </Copyable>
              )}
            </div>
            <div className="text-fg-secondary mt-1.5 space-y-0.5 text-xs select-text">
              {r.name && <p>{r.name}</p>}
              {r.provider && <p className="text-fg-secondary">{r.provider}</p>}
              {r.departure_airport && r.arrival_airport && (
                <p className="font-medium">
                  {r.departure_airport} → {r.arrival_airport}
                  {r.flight_number && (
                    <>
                      {' '}
                      (<Copyable value={r.flight_number}>{r.flight_number}</Copyable>)
                    </>
                  )}
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
          <div className="border-border bg-bg-secondary rounded-lg border p-4" key={`ord-${i}`}>
            <div className="flex items-center gap-2">
              <ShoppingBag className="text-success h-4 w-4" />
              <span className="text-fg-secondary text-xs font-medium">Order</span>
              {o.order_number && (
                <Copyable value={o.order_number}>
                  <span className="bg-border text-fg-secondary md:text-mini rounded px-1.5 py-0.5 font-mono text-xs">
                    #{o.order_number}
                  </span>
                </Copyable>
              )}
              {o.merchant && <span className="text-fg-secondary text-xs">{o.merchant}</span>}
            </div>
            {o.items.length > 0 && (
              <ul className="text-fg-secondary mt-1.5 space-y-0.5 text-xs select-text">
                {o.items.map((item, j) => (
                  <li className="flex items-center gap-2" key={j}>
                    <span>{item.name}</span>
                    {item.quantity && item.quantity > 1 && (
                      <span className="text-fg-muted">x{item.quantity}</span>
                    )}
                    {item.price && <span className="text-fg-secondary">{item.price}</span>}
                  </li>
                ))}
              </ul>
            )}
            {o.total && (
              <p className="text-fg-secondary mt-1.5 text-xs font-medium select-text">
                Total: {o.currency && `${o.currency} `}
                {o.total}
              </p>
            )}
          </div>
        ))}

        {events.map((e, i) => (
          <div className="border-border bg-bg-secondary rounded-lg border p-4" key={`evt-${i}`}>
            <div className="flex items-center gap-2">
              <Calendar className="text-accent h-4 w-4" />
              <span className="text-fg-secondary text-xs font-medium select-text">{e.name}</span>
            </div>
            <div className="text-fg-secondary mt-1 space-y-0.5 text-xs select-text">
              {e.start_date && (
                <p>
                  {formatSchemaDate(e.start_date)}
                  {e.end_date && ` — ${formatSchemaDate(e.end_date)}`}
                </p>
              )}
              {e.location && <p>{e.location}</p>}
              {e.url && isSafeUrl(e.url) && (
                <a
                  className="text-accent hover:underline"
                  href={e.url}
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  Details
                </a>
              )}
            </div>
          </div>
        ))}

        {actions.map((a, i) => (
          <div className="border-border bg-bg-secondary rounded-lg border p-4" key={`act-${i}`}>
            <div className="flex items-center gap-2">
              <ExternalLink className="text-accent h-4 w-4" />
              <span className="text-fg-secondary text-xs font-medium">
                {a.type || 'Action'}: {a.name}
              </span>
            </div>
            {a.url && isSafeUrl(a.url) && (
              <a
                className="text-accent mt-1 inline-block text-xs hover:underline"
                href={a.url}
                rel="noopener noreferrer"
                target="_blank"
              >
                {a.name || 'Open'}
              </a>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}

function formatSchemaDate(dateStr: string): string {
  try {
    const d = new Date(dateStr)
    if (isNaN(d.getTime())) return dateStr
    return d.toLocaleDateString(undefined, {
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      month: 'short',
      year: 'numeric',
    })
  } catch {
    return dateStr
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

function ReservationIcon({ kind }: { kind: string }) {
  switch (kind) {
    case 'flight':
      return <Plane className="text-info h-4 w-4" />
    case 'hotel':
      return <Building2 className="text-warning h-4 w-4" />
    case 'restaurant':
      return <UtensilsCrossed className="text-warning h-4 w-4" />
    default:
      return <Car className="text-accent h-4 w-4" />
  }
}
