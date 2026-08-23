package jp.golia.mailrs.wire

/**
 * Server settings somebody types in themselves.
 *
 * Autodiscovery covers the providers people use; this is for the ones
 * it cannot reach — a company IMAP server, a self-hosted one, anything
 * with no SRV record and no entry in the ISPDB.
 *
 * The port stays text because a partially typed number is not a
 * number, and an empty box must not go out as a real port of zero.
 */
data class ManualEndpoint(
    val host: String = "",
    val port: String = "",
    val proto: String,
    val tls: String = "implicit",
)

/**
 * One endpoint as the server wants it, or null.
 *
 * Digits only, deliberately: `"+993".toIntOrNull()` is 993 and
 * `" 993 "` is null, and neither is what somebody typing a port means.
 */
fun wireEndpoint(e: ManualEndpoint): WireEndpoint? {
    val host = e.host.trim()
    val typed = e.port.trim()
    if (host.isEmpty() || typed.isEmpty() || !typed.all { it.isDigit() }) return null
    val port = typed.toIntOrNull() ?: return null
    if (port !in 1..65535) return null
    return WireEndpoint(host = host, port = port, protocol = e.proto, tls = e.tls)
}

/**
 * Both endpoints, or null — a half-filled pair is refused by the
 * server with a validation error rather than a hint, so it never
 * leaves the phone.
 */
fun wireEndpoints(incoming: ManualEndpoint, outgoing: ManualEndpoint): Pair<WireEndpoint, WireEndpoint>? {
    val i = wireEndpoint(incoming) ?: return null
    val o = wireEndpoint(outgoing) ?: return null
    return i to o
}
