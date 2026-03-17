import { Building2, Calendar, Car, ExternalLink, Plane, ShoppingBag, UtensilsCrossed } from 'lucide-react'
import { Copyable } from '@/components/copy-button'
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
    <div className="border-b border-[var(--color-border-default)] px-5 py-3">
      <p className="mb-2 text-xs font-medium uppercase tracking-wider text-[var(--color-text-tertiary)]">
        Structured Data
      </p>
      <div className="space-y-2">
        {reservations.map((r, i) => (
          <div
            key={`res-${i}`}
            className="border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4"
          >
            <div className="flex items-center gap-2">
              <ReservationIcon kind={r.type} />
              <span className="text-xs font-medium capitalize text-[var(--color-text-secondary)]">
                {r.type} Reservation
              </span>
              {r.reservation_id && (
                <Copyable value={r.reservation_id}>
                  <span className="bg-[var(--color-border-default)] px-1.5 py-0.5 text-[11px] font-mono text-[var(--color-text-secondary)]">
                    {r.reservation_id}
                  </span>
                </Copyable>
              )}
            </div>
            <div className="mt-1.5 select-text space-y-0.5 text-xs text-[var(--color-text-secondary)]">
              {r.name && <p>{r.name}</p>}
              {r.provider && <p className="text-[var(--color-text-secondary)]">{r.provider}</p>}
              {r.departure_airport && r.arrival_airport && (
                <p className="font-medium">
                  {r.departure_airport} → {r.arrival_airport}
                  {r.flight_number && (
                    <> (<Copyable value={r.flight_number}>{r.flight_number}</Copyable>)</>
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
          <div
            key={`ord-${i}`}
            className="border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4"
          >
            <div className="flex items-center gap-2">
              <ShoppingBag className="h-4 w-4 text-[var(--color-status-success)]" />
              <span className="text-xs font-medium text-[var(--color-text-secondary)]">
                Order
              </span>
              {o.order_number && (
                <Copyable value={o.order_number}>
                  <span className="bg-[var(--color-border-default)] px-1.5 py-0.5 text-[11px] font-mono text-[var(--color-text-secondary)]">
                    #{o.order_number}
                  </span>
                </Copyable>
              )}
              {o.merchant && (
                <span className="text-xs text-[var(--color-text-secondary)]">{o.merchant}</span>
              )}
            </div>
            {o.items.length > 0 && (
              <ul className="mt-1.5 select-text space-y-0.5 text-xs text-[var(--color-text-secondary)]">
                {o.items.map((item, j) => (
                  <li key={j} className="flex items-center gap-2">
                    <span>{item.name}</span>
                    {item.quantity && item.quantity > 1 && (
                      <span className="text-[var(--color-text-tertiary)]">x{item.quantity}</span>
                    )}
                    {item.price && (
                      <span className="text-[var(--color-text-secondary)]">{item.price}</span>
                    )}
                  </li>
                ))}
              </ul>
            )}
            {o.total && (
              <p className="mt-1.5 select-text text-xs font-medium text-[var(--color-text-secondary)]">
                Total: {o.currency && `${o.currency} `}{o.total}
              </p>
            )}
          </div>
        ))}

        {events.map((e, i) => (
          <div
            key={`evt-${i}`}
            className="border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4"
          >
            <div className="flex items-center gap-2">
              <Calendar className="h-4 w-4 text-[var(--color-brand-primary)]" />
              <span className="select-text text-xs font-medium text-[var(--color-text-secondary)]">
                {e.name}
              </span>
            </div>
            <div className="mt-1 select-text space-y-0.5 text-xs text-[var(--color-text-secondary)]">
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
                  className="text-[var(--color-brand-primary)] hover:underline"
                >
                  Details
                </a>
              )}
            </div>
          </div>
        ))}

        {actions.map((a, i) => (
          <div key={`act-${i}`} className="border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4">
            <div className="flex items-center gap-2">
              <ExternalLink className="h-4 w-4 text-[var(--color-brand-primary)]" />
              <span className="text-xs font-medium text-[var(--color-text-secondary)]">
                {a.type || 'Action'}: {a.name}
              </span>
            </div>
            {a.url && isSafeUrl(a.url) && (
              <a href={a.url} target="_blank" rel="noopener noreferrer"
                className="mt-1 inline-block text-xs text-[var(--color-brand-primary)] hover:underline">
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
      return <Plane className="h-4 w-4 text-[var(--color-status-info)]" />
    case 'hotel':
      return <Building2 className="h-4 w-4 text-[var(--color-status-warning)]" />
    case 'restaurant':
      return <UtensilsCrossed className="h-4 w-4 text-[var(--color-status-warning)]" />
    default:
      return <Car className="h-4 w-4 text-[var(--color-brand-primary)]" />
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
