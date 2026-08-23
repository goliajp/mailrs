import Testing

@testable import Mailrs

/// Server settings somebody typed in themselves.
///
/// The web, this app and the Android app each shape these before
/// sending, and they must agree: a half-filled pair is refused by the
/// server with a validation error rather than a hint, so it should
/// never leave the phone at all.
@Suite struct ManualEndpointsTests {
    private func e(_ host: String, _ port: String, _ proto: String = "imap") -> ManualEndpoint {
        ManualEndpoint(host: host, port: port, proto: proto)
    }

    @Test func bothEndpointsGoOutWhenBothAreComplete() {
        let out = wireEndpoints(incoming: e("imap.x.jp", "993"), outgoing: e("smtp.x.jp", "465", "smtp"))
        #expect(out?["incoming"] as? [String: Any] != nil)
        let i = out?["incoming"] as? [String: Any]
        #expect(i?["host"] as? String == "imap.x.jp")
        #expect(i?["port"] as? Int == 993)
        #expect(i?["protocol"] as? String == "imap")
    }

    /// An empty box must not become a real port.
    @Test func anEmptyPortIsRefusedRatherThanSent() {
        #expect(wireEndpoints(incoming: e("imap.x.jp", ""), outgoing: e("smtp.x.jp", "465", "smtp")) == nil)
    }

    @Test func aHalfFilledPairNeverLeavesThePhone() {
        #expect(wireEndpoints(incoming: e("", "993"), outgoing: e("smtp.x.jp", "465", "smtp")) == nil)
        #expect(wireEndpoints(incoming: e("imap.x.jp", "993"), outgoing: e("", "465", "smtp")) == nil)
    }

    @Test func aPortOutsideTheRangeIsRefused() {
        for p in ["0", "65536", "99999"] {
            #expect(wireEndpoint(e("h", p)) == nil, "port \(p) was accepted")
        }
    }

    @Test func onlyDigitsCountAsAPort() {
        for p in ["+993", "9 9", "99.5", "abc", "1e3"] {
            #expect(wireEndpoint(e("h", p)) == nil, "port \(p) was accepted")
        }
    }

    /// Spaces around a port are somebody's paste, not a mistake.
    @Test func spacesAroundAPortAreTrimmed() {
        #expect(wireEndpoint(e("h", " 993 "))?["port"] as? Int == 993)
    }

    @Test func theProtocolAndTheEncryptionSurvive() {
        let out = wireEndpoint(ManualEndpoint(host: "pop.x.jp", port: "110", proto: "pop3", tls: "starttls"))
        #expect(out?["protocol"] as? String == "pop3")
        #expect(out?["tls"] as? String == "starttls")
    }
}
