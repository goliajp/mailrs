import Foundation

/// Whether to fetch a whole message or only its beginning.
///
/// Opening a message costs what the message weighs. One with a 25 MB
/// attachment is 25 MB to fetch, and fetching it to show two lines of
/// text — on somebody's mobile data, without asking — is the kind of
/// thing a person notices on their bill rather than on their screen.
///
/// Pure, because the decision is the part worth arguing with, and
/// because the **honesty rule** below is easy to drop: a client that
/// fetches part of a message and does not say so shows a message with
/// its attachments missing and no explanation.
enum FetchWhole {
    /// Past this, ask before fetching everything.
    static let threshold: Int64 = 1_000_000

    /// How much of a large message to take on first open.
    static let preview: Int64 = 262_144

    enum Plan: Equatable {
        /// Small enough, or the reader asked for all of it.
        case whole
        /// The first `bytes` only.
        ///
        /// **The caller must say so.** The text will usually be
        /// complete — it comes before the attachments in nearly every
        /// message — but the attachment list will not be, and a list
        /// that is silently short is worse than one that is absent.
        case beginning(bytes: Int64)
    }

    /// - Parameters:
    ///   - size: what the server said the message weighs, or nil when
    ///     it did not say. **Nil fetches whole**: a message of unknown
    ///     size is usually a small one, and refusing to show it
    ///     properly on a guess is worse than the fetch.
    ///   - askedForAll: set when the reader has pressed the button.
    static func decide(size: Int64?, askedForAll: Bool = false) -> Plan {
        if askedForAll { return .whole }
        guard let size else { return .whole }
        if size <= threshold { return .whole }
        return .beginning(bytes: preview)
    }

    /// The `BODY.PEEK[]` argument for a plan.
    ///
    /// `<0.262144>` is RFC 3501's partial fetch: offset then length.
    /// The offset is written even though it is zero, because the form
    /// without it means something else — the whole body.
    static func bodyItem(_ plan: Plan) -> String {
        switch plan {
        case .whole: "BODY.PEEK[]"
        case let .beginning(bytes): "BODY.PEEK[]<0.\(bytes)>"
        }
    }
}
