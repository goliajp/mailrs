import Foundation

/// Fetching one message to read it.
///
/// Bodies are not stored — they are fetched when a message is opened
/// and kept only while it is on screen. A phone that keeps every body
/// it has ever shown fills up, and the ones worth keeping are the ones
/// somebody chose to keep.
enum MessageReader {
    struct Loaded: Equatable {
        /// Always text by the time it gets here: markup is turned into
        /// text rather than rendered, so no message can ask another
        /// server for an image and report that it was read.
        var text: String
        /// Whether the text came out of markup. Shown, because a
        /// message that reads oddly is easier to forgive when it says
        /// where it came from.
        var fromHTML: Bool
        /// The message's own headers, for replying.
        ///
        /// Read here rather than from the list row: the row has what a
        /// row shows, and a reply needs `Reply-To` and `References`,
        /// which no list has ever displayed.
        var headers = MessageHeaders.Parsed()
    }

    /// What a reader gets: the message, or a sentence about why not.
    enum Outcome: Equatable {
        case loaded(Loaded)
        case failed(String)
    }

    static func load(account: MailAccount, row: MailboxRow) async -> Outcome {
        guard let secret = AccountStore.secret(for: account.id) else {
            return .failed("Sign in again to read this account")
        }
        let session = IMAPSession(host: account.imapHost, port: account.imapPort)
        do {
            try await session.connect()
            if account.auth == .oauth2 {
                try await session.authenticateXOAuth2(user: account.loginName, token: secret)
            } else {
                try await session.login(user: account.loginName, password: secret)
            }
            _ = try await session.select(row.folder)
            let raw = try await session.fetchRaw(uid: row.uid)
            // Marked read only after the body is in hand: a fetch that
            // fails should leave the message unread, or a server that
            // was briefly unwell quietly empties somebody's unread
            // count.
            if !row.seen {
                try? await session.markSeen(uid: row.uid)
                AccountStore.saveRows(MailboxApply.markSeen(AccountStore.rows(), id: row.id))
            }
            await session.close()
            return .loaded(display(of: raw))
        } catch {
            await session.close()
            return .failed("Could not open this message")
        }
    }

    /// The body, as text.
    static func display(of raw: Data) -> Loaded {
        let body = MessageBody.extract(raw)
        let headers = MessageHeaders.parse(String(decoding: raw, as: UTF8.self))
        guard body.isHTML else {
            return Loaded(text: body.text, fromHTML: false, headers: headers)
        }
        return Loaded(text: HTMLText.plain(body.text), fromHTML: true, headers: headers)
    }
}
