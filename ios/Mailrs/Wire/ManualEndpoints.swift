import Foundation

/// Server settings somebody types in themselves.
///
/// Autodiscovery covers the providers people use; this is for the ones
/// it cannot reach — a company IMAP server, a self-hosted one,
/// anything with no SRV record and no entry in the ISPDB.
struct ManualEndpoint: Equatable {
    var host: String = ""
    /// Kept as text, because a partially typed number is not a number.
    var port: String = ""
    var proto: String
    var tls: String = "implicit"
}

/// One endpoint as the server wants it, or nothing.
///
/// Surrounding spaces are trimmed — somebody pasting " 993 " meant
/// 993 — and what is left must be digits: `Int("+993")` is 993, which
/// is not what somebody typing a port means.
func wireEndpoint(_ e: ManualEndpoint) -> [String: Any]? {
    let host = e.host.trimmingCharacters(in: .whitespaces)
    let typed = e.port.trimmingCharacters(in: .whitespaces)
    guard !host.isEmpty,
          !typed.isEmpty,
          typed.allSatisfy(\.isNumber),
          let port = Int(typed),
          (1...65535).contains(port)
    else { return nil }
    return ["host": host, "port": port, "protocol": e.proto, "tls": e.tls]
}

/// Both endpoints, or nothing — a half-filled pair is refused by the
/// server with a validation error rather than a hint, so it never
/// leaves the phone.
func wireEndpoints(incoming: ManualEndpoint, outgoing: ManualEndpoint) -> [String: Any]? {
    guard let i = wireEndpoint(incoming), let o = wireEndpoint(outgoing) else { return nil }
    return ["incoming": i, "outgoing": o]
}
