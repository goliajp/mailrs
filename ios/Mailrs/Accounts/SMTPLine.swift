import Foundation

/// Reading what an SMTP server says, and the two AUTH payloads.
///
/// Split from the socket for the same reason as `IMAP`: the mistakes
/// worth making are in the grammar, and a test that needs a server is
/// a test nobody runs.
enum SMTP {
    /// One reply: a code, and whether more lines follow.
    struct Reply: Equatable {
        let code: Int
        let text: String
        /// `250-STARTTLS` continues; `250 OK` ends.
        let more: Bool

        var isPositive: Bool { (200..<400).contains(code) }
        /// 4xx is "try later", 5xx is "do not try again".
        var isPermanent: Bool { (500..<600).contains(code) }
    }

    /// Read one reply line.
    ///
    /// The fourth character decides: `-` means another line follows,
    /// a space means this was the last. Getting that wrong reads the
    /// next command's reply as this one's.
    static func reply(_ line: String) -> Reply? {
        let t = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard t.count >= 3, let code = Int(t.prefix(3)) else { return nil }
        if t.count == 3 { return Reply(code: code, text: "", more: false) }
        let sep = t[t.index(t.startIndex, offsetBy: 3)]
        guard sep == "-" || sep == " " else { return nil }
        return Reply(
            code: code,
            text: String(t.dropFirst(4)),
            more: sep == "-")
    }

    /// `AUTH PLAIN` — RFC 4616.
    ///
    /// Authorisation identity, authentication identity and password,
    /// separated by **NUL**, then base64. The separator is the trap:
    /// spaces authenticate as nobody and the server answers with what
    /// reads as a wrong password. The authorisation identity is left
    /// empty — repeating the username there is accepted by some
    /// servers and refused by Gmail.
    static func authPlain(user: String, password: String) -> String {
        var raw = Data([0])
        raw.append(contentsOf: Array(user.utf8))
        raw.append(0)
        raw.append(contentsOf: Array(password.utf8))
        return raw.base64EncodedString()
    }

    /// `AUTH XOAUTH2`.
    ///
    /// Not `AUTH PLAIN` with a different secret: `\u{1}` separators, an
    /// `auth=Bearer ` prefix, and two terminators. An access token sent
    /// through `AUTH PLAIN` is refused, and the person is told their
    /// password is wrong for an account whose credentials are fine.
    static func authXOAuth2(user: String, token: String) -> String {
        Data("user=\(user)\u{1}auth=Bearer \(token)\u{1}\u{1}".utf8)
            .base64EncodedString()
    }

    /// Dot-stuffing — RFC 5321 §4.5.2.
    ///
    /// A line of the message that begins with `.` would otherwise end
    /// the DATA block, truncating the message at that line. Every mail
    /// client has shipped this bug at least once; the symptom is a
    /// message that arrives cut in half.
    static func dotStuffed(_ body: String) -> String {
        body
            .replacingOccurrences(of: "\r\n", with: "\n")
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map { $0.hasPrefix(".") ? "." + $0 : String($0) }
            .joined(separator: "\r\n")
    }

    /// Whether a refusal means the credential is wrong.
    ///
    /// 535 is the code; some servers only say it in the text. One is a
    /// button to press, the other is waiting.
    static func isAuthenticationFailure(code: Int, text: String) -> Bool {
        if code == 535 { return true }
        let t = text.uppercased()
        return t.contains("AUTHENTICATION FAILED")
            || t.contains("INVALID CREDENTIALS")
            || t.contains("USERNAME AND PASSWORD NOT ACCEPTED")
    }
}
