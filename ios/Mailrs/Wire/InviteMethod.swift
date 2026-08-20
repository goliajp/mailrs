import Foundation
import SwiftUI

/// What kind of invitation this is, and whether it wants an answer.
///
/// The same three rules the web applies, kept here rather than in the
/// view so they can be tested without a screen — and stated once per
/// platform rather than guessed at twice.
enum InviteMethod {
    /// The label above the card.
    ///
    /// `METHOD:UPDATE` exists in RFC 5546 and almost nobody sends it:
    /// Exchange re-sends the whole invitation as a `REQUEST` with a
    /// higher `SEQUENCE`, which is how a meeting that has moved three
    /// times arrives. Calling that a new invitation tells the reader
    /// the opposite of what happened.
    static func badge(_ method: String, sequence: Int) -> LocalizedStringKey {
        switch method.uppercased() {
        case "CANCEL": return "Cancelled"
        case "COUNTER": return "Counter-proposed"
        case "PUBLISH": return "Event"
        case "REPLY": return "Reply"
        case "REQUEST": return sequence > 0 ? "Updated invite" : "New invite"
        case "UPDATE": return "Updated invite"
        default: return "Invitation"
        }
    }

    /// Whether to offer Yes / Maybe / No.
    ///
    /// Only a `REQUEST` asks the reader anything. A `PUBLISH` is a
    /// notice and a `REPLY` is somebody else's answer arriving;
    /// answering either sends an iTIP message to a party that did not
    /// ask for one.
    static func wantsAnswer(_ method: String) -> Bool {
        method.uppercased() == "REQUEST"
    }

    /// Whether the organiser's zone is worth naming beside the
    /// reader's.
    ///
    /// Exchange writes Windows zone names, which `TimeZone` does not
    /// know, so the comparison is against the handful that turn up plus
    /// the IANA name itself.
    static func zoneDiffers(_ zone: String) -> Bool {
        let reader = TimeZone.current.identifier
        if zone == reader { return false }
        return windowsToIana[zone] != reader
    }

    private static let windowsToIana: [String: String] = [
        "Central Standard Time": "America/Chicago",
        "China Standard Time": "Asia/Shanghai",
        "Eastern Standard Time": "America/New_York",
        "GMT Standard Time": "Europe/London",
        "Pacific Standard Time": "America/Los_Angeles",
        "Tokyo Standard Time": "Asia/Tokyo",
        "W. Europe Standard Time": "Europe/Berlin",
    ]
}

/// Who is coming, in a line.
enum InviteGuests {
    /// "4 guests · 1 yes, 2 awaiting" — the count alone does not say
    /// whether the meeting is happening.
    static func summary(_ attendees: [Wire.Invite.Attendee]) -> String {
        let head = attendees.count == 1 ? "1 guest" : "\(attendees.count) guests"
        let yes = attendees.filter { $0.partstat.uppercased() == "ACCEPTED" }.count
        let no = attendees.filter { $0.partstat.uppercased() == "DECLINED" }.count
        let waiting = attendees.filter { $0.partstat.uppercased() == "NEEDS-ACTION" }.count
        var parts: [String] = []
        if yes > 0 { parts.append("\(yes) yes") }
        if no > 0 { parts.append("\(no) no") }
        if waiting > 0 { parts.append("\(waiting) awaiting") }
        return parts.isEmpty ? head : "\(head) · \(parts.joined(separator: ", "))"
    }

    /// What this reader already said.
    static func answered(_ partstat: String) -> String {
        switch partstat.uppercased() {
        case "ACCEPTED": return "You accepted"
        case "DECLINED": return "You declined"
        case "TENTATIVE": return "You answered maybe"
        default: return "You answered"
        }
    }
}
