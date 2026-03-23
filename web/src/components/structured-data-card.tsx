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
    reservations.length > 0 ||
    orders.length > 0 ||
    events.length > 0 ||
    actions.length > 0

  if (!hasContent) return null

  return (
    <div className="border-b border-[var(--color-border-default)] px-5 py-3">
      <p className="mb-2 text-xs font-medium tracking-wider text-[var(--color-text-tertiary)] uppercase">
        Structured Data
      </p>
      <div className="space-y-2">
        {reservations.map((r, i) => (
          <div
            className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4"
            key={`res-${i}`}
          >
            <div className="flex items-center gap-2">
              <ReservationIcon kind={r.type} />
              <span className="text-xs font-medium text-[var(--color-text-secondary)] capitalize">
                {r.type} Reservation
              </span>
              {r.reservation_id && (
                <Copyable value={r.reservation_id}>
                  <span className="rounded bg-[var(--color-border-default)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--color-text-secondary)]">
                    {r.reservation_id}
                  </span>
                </Copyable>
              )}
            </div>
            <div className="mt-1.5 space-y-0.5 text-xs text-[var(--color-text-secondary)] select-text">
              {r.name && <p>{r.name}</p>}
              {r.provider && (
                <p className="text-[var(--color-text-secondary)]">
                  {r.provider}
                </p>
              )}
              {r.departure_airport && r.arrival_airport && (
                <p className="font-medium">
                  {r.departure_airport} → {r.arrival_airport}
                  {r.flight_number && (
                    <>
                      {' '}
                      (
                      <Copyable value={r.flight_number}>
                        {r.flight_number}
                      </Copyable>
                      )
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
          <div
            className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4"
            key={`ord-${i}`}
          >
            <div className="flex items-center gap-2">
              <ShoppingBag className="h-4 w-4 text-[var(--color-status-success)]" />
              <span className="text-xs font-medium text-[var(--color-text-secondary)]">
                Order
              </span>
              {o.order_number && (
                <Copyable value={o.order_number}>
                  <span className="rounded bg-[var(--color-border-default)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--color-text-secondary)]">
                    #{o.order_number}
                  </span>
                </Copyable>
              )}
              {o.merchant && (
                <span className="text-xs text-[var(--color-text-secondary)]">
                  {o.merchant}
                </span>
              )}
            </div>
            {o.items.length > 0 && (
              <ul className="mt-1.5 space-y-0.5 text-xs text-[var(--color-text-secondary)] select-text">
                {o.items.map((item, j) => (
                  <li className="flex items-center gap-2" key={j}>
                    <span>{item.name}</span>
                    {item.quantity && item.quantity > 1 && (
                      <span className="text-[var(--color-text-tertiary)]">
                        x{item.quantity}
                      </span>
                    )}
                    {item.price && (
                      <span className="text-[var(--color-text-secondary)]">
                        {item.price}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
            {o.total && (
              <p className="mt-1.5 text-xs font-medium text-[var(--color-text-secondary)] select-text">
                Total: {o.currency && `${o.currency} `}
                {o.total}
              </p>
            )}
          </div>
        ))}

        {events.map((e, i) => (
          <div
            className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4"
            key={`evt-${i}`}
          >
            <div className="flex items-center gap-2">
              <Calendar className="h-4 w-4 text-[var(--color-brand-primary)]" />
              <span className="text-xs font-medium text-[var(--color-text-secondary)] select-text">
                {e.name}
              </span>
            </div>
            <div className="mt-1 space-y-0.5 text-xs text-[var(--color-text-secondary)] select-text">
              {e.start_date && (
                <p>
                  {formatSchemaDate(e.start_date)}
                  {e.end_date && ` — ${formatSchemaDate(e.end_date)}`}
                </p>
              )}
              {e.location && <p>{e.location}</p>}
              {e.url && isSafeUrl(e.url) && (
                <a
                  className="text-[var(--color-brand-primary)] hover:underline"
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
          <div
            className="rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] p-4"
            key={`act-${i}`}
          >
            <div className="flex items-center gap-2">
              <ExternalLink className="h-4 w-4 text-[var(--color-brand-primary)]" />
              <span className="text-xs font-medium text-[var(--color-text-secondary)]">
                {a.type || 'Action'}: {a.name}
              </span>
            </div>
            {a.url && isSafeUrl(a.url) && (
              <a
                className="mt-1 inline-block text-xs text-[var(--color-brand-primary)] hover:underline"
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
      return <Plane className="h-4 w-4 text-[var(--color-status-info)]" />
    case 'hotel':
      return (
        <Building2 className="h-4 w-4 text-[var(--color-status-warning)]" />
      )
    case 'restaurant':
      return (
        <UtensilsCrossed className="h-4 w-4 text-[var(--color-status-warning)]" />
      )
    default:
      return <Car className="h-4 w-4 text-[var(--color-brand-primary)]" />
  }
}
