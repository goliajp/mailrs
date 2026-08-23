import type { ManualEndpoint } from '@/lib/manual-endpoints'

import { inputClass } from './_shared'

/**
 * Where a server is, when nothing can work it out.
 *
 * Autodiscovery covers the providers people actually use, and a form
 * that opens with eight empty boxes teaches everybody that connecting
 * mail is hard. So this stays shut until somebody says it is needed —
 * and then it asks for everything at once, because a half-filled
 * endpoint is refused with a validation error rather than a hint.
 *
 * The protocols are the ones the sync worker can read: IMAP, POP3 and
 * JMAP in, SMTP out.
 */
const INCOMING = ['imap', 'pop3', 'jmap']
const TLS = [
  { label: 'TLS from the first byte', value: 'implicit' },
  { label: 'STARTTLS', value: 'starttls' },
  { label: 'None (not recommended)', value: 'none' },
]

export function ManualServerFields({
  incoming,
  onIncoming,
  onOutgoing,
  onUsername,
  outgoing,
  username,
}: {
  incoming: ManualEndpoint
  onIncoming: (e: ManualEndpoint) => void
  onOutgoing: (e: ManualEndpoint) => void
  onUsername: (v: string) => void
  outgoing: ManualEndpoint
  username: string
}) {
  return (
    <div className="border-border space-y-3 rounded-md border p-3">
      <EndpointFields
        endpoint={incoming}
        label="Incoming"
        onChange={onIncoming}
        protocols={INCOMING}
      />
      <EndpointFields
        endpoint={outgoing}
        label="Outgoing"
        onChange={onOutgoing}
        protocols={['smtp']}
      />
      <input
        aria-label="Login name"
        className={inputClass}
        onChange={(e) => onUsername(e.target.value)}
        placeholder="Login name, if it is not the address"
        type="text"
        value={username}
      />
    </div>
  )
}

function EndpointFields({
  endpoint,
  label,
  onChange,
  protocols,
}: {
  endpoint: ManualEndpoint
  label: string
  onChange: (e: ManualEndpoint) => void
  protocols: string[]
}) {
  return (
    <div className="space-y-2">
      <p className="text-fg-muted text-xs">{label}</p>
      <div className="flex gap-2">
        <input
          aria-label={`${label} server`}
          className={`${inputClass} flex-1`}
          onChange={(e) => onChange({ ...endpoint, host: e.target.value })}
          placeholder={label === 'Incoming' ? 'imap.example.com' : 'smtp.example.com'}
          type="text"
          value={endpoint.host}
        />
        <input
          aria-label={`${label} port`}
          className={`${inputClass} w-20`}
          inputMode="numeric"
          onChange={(e) => onChange({ ...endpoint, port: e.target.value })}
          placeholder="993"
          type="text"
          value={endpoint.port}
        />
      </div>
      <div className="flex gap-2">
        {protocols.length > 1 && (
          <select
            aria-label={`${label} protocol`}
            className={`${inputClass} flex-1`}
            onChange={(e) => onChange({ ...endpoint, protocol: e.target.value })}
            value={endpoint.protocol}
          >
            {protocols.map((p) => (
              <option key={p} value={p}>
                {p.toUpperCase()}
              </option>
            ))}
          </select>
        )}
        <select
          aria-label={`${label} encryption`}
          className={`${inputClass} flex-1`}
          onChange={(e) => onChange({ ...endpoint, tls: e.target.value })}
          value={endpoint.tls}
        >
          {TLS.map((t) => (
            <option key={t.value} value={t.value}>
              {t.label}
            </option>
          ))}
        </select>
      </div>
    </div>
  )
}
