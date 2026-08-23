import Foundation

/// Reading a JMAP mailbox — RFC 8620 / 8621.
///
/// Ordinary HTTPS rather than a socket, so this needs no session actor
/// of its own. What it does need is the two shapes a JMAP client can
/// get wrong: finding the API url and the account id in the session
/// object, and knowing when `Email/changes` has given up.
enum JMAP {
    /// What `/.well-known/jmap` answers, as far as this client uses it.
    struct Session: Equatable {
        /// Where every request goes.
        let apiUrl: String
        /// The account whose mail this is.
        ///
        /// A person may have several — a personal one and one their
        /// employer owns — and the primary account for mail is the one
        /// named under the mail capability, not the first in the map.
        let accountId: String
    }

    static let mailCapability = "urn:ietf:params:jmap:mail"

    /// Read the session object.
    ///
    /// `primaryAccounts` is what names the mail account; picking the
    /// first key of `accounts` instead works until somebody has two,
    /// and then it silently reads the wrong mailbox.
    static func session(_ data: Data) -> Session? {
        guard let top = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let apiUrl = top["apiUrl"] as? String, !apiUrl.isEmpty
        else { return nil }
        if let primary = top["primaryAccounts"] as? [String: Any],
           let id = primary[mailCapability] as? String, !id.isEmpty {
            return Session(apiUrl: apiUrl, accountId: id)
        }
        // A server with exactly one account and no primaryAccounts is
        // unambiguous; more than one without it is not, and guessing
        // there would read somebody else's mailbox.
        if let accounts = top["accounts"] as? [String: Any], accounts.count == 1,
           let id = accounts.keys.first {
            return Session(apiUrl: apiUrl, accountId: id)
        }
        return nil
    }

    /// What a `Email/changes` response means for the caller.
    enum Changes: Equatable {
        /// Ids that arrived, and the state to ask from next time.
        case some(created: [String], newState: String)
        /// The server cannot answer from this state.
        ///
        /// **Not an error.** RFC 8620 §5.2: the client is told to
        /// start over with `Email/query`, and treating it as a failure
        /// leaves an account that never syncs again.
        case startOver
    }

    /// Read a `Email/changes` reply.
    static func changes(_ data: Data) -> Changes? {
        guard let top = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let responses = top["methodResponses"] as? [[Any]],
              let first = responses.first, first.count >= 2
        else { return nil }
        let name = first[0] as? String ?? ""
        guard let body = first[1] as? [String: Any] else { return nil }
        if name == "error" {
            let type = body["type"] as? String ?? ""
            return type == "cannotCalculateChanges" ? .startOver : nil
        }
        guard let newState = body["newState"] as? String else { return nil }
        let created = body["created"] as? [String] ?? []
        return .some(created: created, newState: newState)
    }

    /// The newest messages, in **one** round trip.
    ///
    /// The back-reference (`#ids`) is what makes it one: it tells the
    /// server to feed the ids from the query straight into the get. A
    /// client that does not use it asks, waits, and asks again — which
    /// on a phone is two of everything, including the latency.
    static func newestRequest(accountId: String, limit: Int = 50) -> String {
        """
        {"using":["urn:ietf:params:jmap:core","\(mailCapability)"],\
        "methodCalls":[\
        ["Email/query",{"accountId":"\(accountId)",\
        "sort":[{"property":"receivedAt","isAscending":false}],\
        "limit":\(limit)},"0"],\
        ["Email/get",{"accountId":"\(accountId)",\
        "#ids":{"resultOf":"0","name":"Email/query","path":"/ids"},\
        "properties":["id","subject","from","receivedAt","keywords","messageId"]},"1"]\
        ]}
        """
    }

    /// One message, as far as a list row needs it.
    struct Email: Equatable {
        var id: String
        var subject: String
        var sender: String
        /// Seconds since the epoch, or nil when `receivedAt` was
        /// unreadable.
        var receivedAt: Int64?
        var seen: Bool
        var messageId: String
    }

    /// Read an `Email/get` reply.
    ///
    /// Three shapes worth naming, because each is silently wrong if
    /// guessed:
    ///
    /// - `from` is a **list of objects**, not a string. Reading it as
    ///   text gives an empty sender on every row.
    /// - `keywords` says what is true, so `$seen` **absent** means
    ///   unread — the same absence that IMAP's flag list uses.
    /// - `receivedAt` is a UTC date string, not a number.
    static func emails(_ data: Data) -> [Email]? {
        guard let top = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let responses = top["methodResponses"] as? [[Any]]
        else { return nil }
        // The get is not always second: a server may answer in any
        // order, and one that pushes a `Core/echo` in front shifts it.
        guard
            let get = responses.first(where: { $0.count >= 2 && $0[0] as? String == "Email/get" }),
            let payload = get[1] as? [String: Any],
            let list = payload["list"] as? [[String: Any]]
        else { return nil }
        return list.map { item in
            let from = (item["from"] as? [[String: Any]])?.first
            let name = from?["name"] as? String ?? ""
            let address = from?["email"] as? String ?? ""
            var sender = name
            if !name.isEmpty, !address.isEmpty { sender = "\(name) <\(address)>" }
            if name.isEmpty { sender = address }
            let keywords = item["keywords"] as? [String: Any]
            return Email(
                id: item["id"] as? String ?? "",
                subject: item["subject"] as? String ?? "",
                sender: sender,
                receivedAt: utcDate(item["receivedAt"] as? String),
                seen: keywords?["$seen"] != nil,
                messageId: (item["messageId"] as? [String])?.first ?? "")
        }
    }

    /// `2026-08-24T01:46:40Z`, to seconds.
    ///
    /// Hand-read rather than handed to a date formatter: JMAP's
    /// UTCDate is one fixed shape, and a formatter would bring a
    /// locale and a default time zone with it — which is how a message
    /// moves by hours for somebody who is not in UTC.
    static func utcDate(_ text: String?) -> Int64? {
        guard let raw = text?.trimmingCharacters(in: .whitespaces), raw.count >= 20,
            raw.hasSuffix("Z")
        else { return nil }
        let c = Array(raw)
        guard c[10] == "T" else { return nil }
        func number(_ from: Int, _ count: Int) -> Int? { Int(String(c[from..<(from + count)])) }
        guard let year = number(0, 4), let month = number(5, 2), let day = number(8, 2),
            let hour = number(11, 2), let minute = number(14, 2), let second = number(17, 2),
            (1...12).contains(month), (1...31).contains(day),
            hour <= 23, minute <= 59, second <= 60
        else { return nil }
        return MailDate.epochFromCivil(
            year: year, month: month, day: day, hour: hour, minute: minute, second: second)
    }
}
