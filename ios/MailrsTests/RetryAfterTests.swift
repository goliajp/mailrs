import Testing

@testable import Mailrs

/// How long a read waits before asking a restarting server again.
@Suite("Retry-After")
struct RetryAfterTests {
    @Test("the server's seconds are used, capped at thirty")
    func secondsAreUsed() {
        #expect(MailrsClient.retryAfterMs("1") == 1_000)
        #expect(MailrsClient.retryAfterMs(" 5 ") == 5_000)
        #expect(MailrsClient.retryAfterMs("3600") == 30_000)
    }

    @Test("no usable value waits five seconds")
    func defaults() {
        #expect(MailrsClient.retryAfterMs(nil) == 5_000)
        #expect(MailrsClient.retryAfterMs("0") == 5_000)
        #expect(MailrsClient.retryAfterMs("-3") == 5_000)
        // the HTTP-date form is not what the server sends; it is not guessed at
        #expect(MailrsClient.retryAfterMs("Wed, 21 Oct 2026 07:28:00 GMT") == 5_000)
    }
}
