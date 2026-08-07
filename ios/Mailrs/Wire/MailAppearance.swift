import Foundation

/// Whether a message's HTML may follow the app's appearance.
///
/// The white slab a dark thread used to show was deliberate: mail is
/// authored against white, and handing a dark background to a message
/// that sets its own black text produces black on black — worse than
/// the slab. But that reasoning only covers mail that styles itself.
/// Most personal mail is a paragraph and a link, and for that the slab
/// is just a bright rectangle in a dark room.
///
/// So the rule is narrow and conservative: a message that declares any
/// colour of its own — a background, a text colour, a `<font>` tag —
/// is a design, and its design is honoured on white paper. Only mail
/// that declares no colour at all inherits the app's.
enum MailAppearance {
    static func followsAppTheme(html: String) -> Bool {
        let lowered = html.lowercased()
        if lowered.contains("bgcolor") { return false }
        if lowered.contains("<font") { return false }
        if lowered.contains("background") { return false }
        return !declaresTextColor(lowered)
    }

    /// `color:` as its own property, not the tail of `border-color:` or
    /// `outline-color:` — a border colour says nothing about whether the
    /// text will be legible.
    private static func declaresTextColor(_ lowered: String) -> Bool {
        var index = lowered.startIndex
        while let found = lowered.range(of: "color:", range: index..<lowered.endIndex) {
            let precededByName: Bool
            if found.lowerBound == lowered.startIndex {
                precededByName = false
            } else {
                let before = lowered[lowered.index(before: found.lowerBound)]
                precededByName = before == "-" || before.isLetter
            }
            if !precededByName { return true }
            index = found.upperBound
        }
        return false
    }
}
