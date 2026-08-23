/**
 * Server settings somebody types in themselves.
 *
 * Autodiscovery covers the providers people use; this is for the ones
 * it cannot reach — a company IMAP server, a self-hosted one, anything
 * with no SRV record and no entry in the ISPDB.
 */
export interface ManualEndpoint {
  host: string
  port: string
  protocol: string
  tls: string
}

export const emptyEndpoint = (protocol: string): ManualEndpoint => ({
  host: '',
  port: '',
  protocol,
  tls: 'implicit',
})

export interface WireEndpoint {
  host: string
  port: number
  protocol: string
  tls: string
}

/**
 * The two endpoints as the server wants them, or nothing.
 *
 * A half-filled endpoint is refused with a validation error rather
 * than a hint, so an incomplete pair never leaves the browser. The
 * port stays a string in the form because a partially typed number is
 * not a number — and `Number('')` is 0 rather than NaN, so an empty
 * box would otherwise be sent as a real port of zero.
 */
export function manualEndpoints(
  incoming: ManualEndpoint,
  outgoing: ManualEndpoint
): null | { incoming: WireEndpoint; outgoing: WireEndpoint } {
  const i = shape(incoming)
  const o = shape(outgoing)
  if (!i || !o) return null
  return { incoming: i, outgoing: o }
}

function shape(e: ManualEndpoint): null | WireEndpoint {
  const host = e.host.trim()
  const typed = e.port.trim()
  // Digits only, deliberately. `Number` accepts '1e3', ' 993' and
  // '+993' as 1000 and 993, which is not what somebody typing a port
  // means — and it reads '' as 0, so an empty box would go out as a
  // real port of zero.
  if (!host || !/^\d+$/.test(typed)) return null
  const port = Number(typed)
  if (port < 1 || port > 65535) return null
  return { host, port, protocol: e.protocol, tls: e.tls }
}
