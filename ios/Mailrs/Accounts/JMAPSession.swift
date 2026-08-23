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
}
