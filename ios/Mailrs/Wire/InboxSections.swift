import Foundation
import SwiftUI

/// The sectioned Inbox's sections this app offers, as the server names
/// them (`crates/mailbox/src/section.rs`): mail waiting on the reader, mail
/// the reader is waiting on, and first contacts.
enum InboxSection: String, CaseIterable, Identifiable, Sendable {
    case reply
    case awaiting
    case new

    var id: String { rawValue }

    var label: LocalizedStringKey {
        switch self {
        case .reply: "To reply"
        case .awaiting: "Waiting for a reply"
        case .new: "New senders"
        }
    }

    /// The senders to accept from the New section: each row's last
    /// participant who is not the reader, once each, lower-cased. The
    /// same rule as the web's `lib/latest-senders.ts`.
    static func latestSenders(_ rows: [Wire.Conversation], me: String) -> [String] {
        let self_ = me.lowercased()
        var out: [String] = []
        for row in rows {
            let addresses = row.participants.map(bare).filter { !$0.isEmpty && $0 != self_ }
            guard let last = addresses.last else { continue }
            if !out.contains(last) { out.append(last) }
        }
        return out
    }

    private static func bare(_ shown: String) -> String {
        if let open = shown.firstIndex(of: "<"), let close = shown.lastIndex(of: ">"), open < close {
            return shown[shown.index(after: open)..<close]
                .trimmingCharacters(in: .whitespaces).lowercased()
        }
        return shown.trimmingCharacters(in: .whitespaces).lowercased()
    }
}
