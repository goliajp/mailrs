import SwiftUI

/// A value, or a stand-in when it is empty.
///
/// The shape was written eight times as `Text(x.isEmpty ? "(no
/// subject)" : x)`, which carries a bug as well as a ternary: the
/// whole expression is a `String`, so `Text` takes its verbatim
/// initialiser and the stand-in never reached a localization table.
/// Here the two branches are separate — the stand-in is a key, the
/// value is verbatim, which is what each of them actually is.
struct ValueOrPlaceholder: View {
    let value: String
    let placeholder: LocalizedStringKey

    var body: some View {
        if value.isEmpty {
            Text(placeholder)
        } else {
            Text(verbatim: value)
        }
    }
}

/// The read/unread swipe action's two faces.
///
/// A swipe action is named for what it will do, not for what the row
/// is — so an unread row offers "Read". Written as a pair of functions
/// because the two names and the two glyphs have to agree, and three
/// conditionals in a row is where they stop agreeing.
enum ReadToggle {
    static func label(unread: Bool) -> LocalizedStringKey {
        if unread { return "Read" }
        return "Unread"
    }

    static func icon(unread: Bool) -> String {
        if unread { return "envelope.open" }
        return "envelope.badge"
    }
}

enum StarToggle {
    static func label(starred: Bool) -> LocalizedStringKey {
        if starred { return "Unstar" }
        return "Star"
    }

    static func icon(starred: Bool) -> String {
        if starred { return "star.slash" }
        return "star"
    }
}
