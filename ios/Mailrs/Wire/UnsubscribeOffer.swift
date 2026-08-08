import Foundation
import SwiftUI

/// What to offer a reader who wants off a list, given what the message
/// said.
///
/// Three answers, not one, because the three cost the reader different
/// things and only one of them is free. Pure so the rule can be read in
/// one place rather than inferred from a chain of `if let`s in a view.
enum UnsubscribeOffer: Equatable {
    /// The server can leave the list on the reader's behalf: one tap,
    /// no browser, nothing of the reader's leaves the server.
    case oneClick

    /// A page to open. The reader's IP and user agent reach the sender
    /// the moment it loads, which is a decision to take deliberately —
    /// so this is offered as a link, never performed on their behalf.
    case openPage(URL)

    /// Only an address to write to. Handed to the composer with the
    /// subject and body the sender asked for, because those are usually
    /// what the address keys on.
    case sendMail(URL)

    /// Nothing usable was advertised.
    case none

    static func of(_ unsubscribe: Wire.Unsubscribe?) -> UnsubscribeOffer {
        guard let unsubscribe else { return .none }
        if unsubscribe.oneClick { return .oneClick }
        // A page before an address: it is one tap against composing and
        // sending a message, and senders who offer both treat them the
        // same.
        for candidate in unsubscribe.http {
            if let url = URL(string: candidate) { return .openPage(url) }
        }
        for candidate in unsubscribe.mailto {
            if let url = URL(string: candidate) { return .sendMail(url) }
        }
        return .none
    }

    var isAvailable: Bool { self != .none }

    /// What the button says. Different words for different costs: the
    /// two that leave the app say so, because a reader who taps
    /// "Unsubscribe" and lands in Safari has been surprised.
    var label: LocalizedStringKey {
        switch self {
        case .oneClick: return "Unsubscribe"
        case .openPage: return "Unsubscribe on the web"
        case .sendMail: return "Unsubscribe by email"
        case .none: return ""
        }
    }
}
