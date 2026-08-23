import Foundation

/// What a reply starts out as.
///
/// Pure, because every one of these decisions is a rule somebody can
/// disagree with, and a rule that can only be checked by sending mail
/// is a rule nobody checks.
enum ReplyDraft {
    /// Reply to one message.
    ///
    /// - `Reply-To` wins over `From` — that is the entire purpose of
    ///   the header, and ignoring it sends replies to a no-reply
    ///   address.
    /// - The subject gains one `Re:` and never a second.
    /// - Threading is carried, or the reply starts a new conversation
    ///   in every client that reads it.
    /// - Parameter all: copy everyone who was already on it. Off by
    ///   default, and a separate button rather than a setting: whether
    ///   the rest of a list should see an answer is a decision per
    ///   message, and a client that decides it once decides it wrong
    ///   half the time.
    static func make(
        to headers: MessageHeaders.Parsed, from account: MailAccount, quoting body: String = "",
        all: Bool = false
    ) -> OutgoingMessage.Draft {
        var chain = headers.references
        if !headers.messageId.isEmpty, !chain.contains(headers.messageId) {
            chain.append(headers.messageId)
        }
        let primary = recipient(headers)
        var copies: [String] = []
        if all {
            copies = MailAddresses.replyAll(
                to: headers.to, cc: headers.cc, primary: primary, mine: account.address)
        }
        return OutgoingMessage.Draft(
            from: account.address,
            fromName: account.displayName,
            to: [primary],
            cc: copies,
            subject: subject(headers.subject),
            body: quoted(body, from: headers),
            inReplyTo: headers.messageId,
            references: chain)
    }

    static func recipient(_ headers: MessageHeaders.Parsed) -> String {
        let replyTo = headers.replyTo.trimmingCharacters(in: .whitespaces)
        if !replyTo.isEmpty { return replyTo }
        return headers.from
    }

    /// One `Re:`, never two.
    ///
    /// A conversation that has been round a few times otherwise reads
    /// `Re: Re: Re: Re:`, and some clients thread on the subject.
    static func subject(_ original: String) -> String {
        let trimmed = original.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return "Re:" }
        var rest = Substring(trimmed)
        // Strip every prefix that is already there, in the forms that
        // actually arrive — including the localised ones, which are
        // what a phone in Japan or China sends.
        var stripped = true
        while stripped {
            stripped = false
            for prefix in ["re:", "re :", "答复:", "回复:", "回覆:"] {
                if rest.lowercased().hasPrefix(prefix) {
                    rest = rest.dropFirst(prefix.count)
                    while rest.first?.isWhitespace == true { rest = rest.dropFirst() }
                    stripped = true
                }
            }
        }
        return "Re: \(rest)"
    }

    /// The original, marked as somebody else's words.
    ///
    /// Empty when there is nothing to quote: a reply that opens with
    /// an attribution line above nothing looks like the message failed
    /// to load.
    static func quoted(_ body: String, from headers: MessageHeaders.Parsed) -> String {
        let text = body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return "" }
        var who = headers.from
        if who.isEmpty { who = "somebody" }
        let lines = text.replacingOccurrences(of: "\r\n", with: "\n")
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map { line -> String in
                // No trailing space on an empty quoted line: it is
                // invisible, and it is what makes a quoted blank line
                // show up as `> ` in the reply.
                if line.isEmpty { return ">" }
                return "> \(line)"
            }
        return "\n\n\(who) wrote:\n" + lines.joined(separator: "\n") + "\n"
    }
}
