import SwiftUI

/// How a conversation row lays itself out at the reader's text size.
///
/// At the accessibility sizes the row's header — name, participant
/// count, message count, date — cannot share one line. Measured on the
/// simulator at `accessibility-extra-extra-extra-large`: the sender
/// rendered as **"A…"** while `8/5/25` kept its full width, and the
/// subject came out as "Quarterly r…". Everything below the second row
/// was empty screen. The layout had not run out of room; it had spent
/// what it had on the least useful thing in the row.
///
/// Pure, because the alternative is a rule you can only see by putting
/// a phone in someone's hand and changing a system setting.
enum RowLayout {
    /// Stack the header vertically instead of fighting over one line.
    static func stacksHeader(_ size: DynamicTypeSize) -> Bool {
        size.isAccessibilitySize
    }

    /// How many lines the sender's name may take.
    ///
    /// One at ordinary sizes — a list is scanned down the left edge and
    /// a wrapping name breaks that edge. Two when a name no longer fits
    /// on one, because a truncated name is not a name.
    static func senderLines(_ size: DynamicTypeSize) -> Int {
        if size.isAccessibilitySize { return 2 }
        return 1
    }

    /// How many lines the subject may take.
    static func subjectLines(_ size: DynamicTypeSize) -> Int {
        if size.isAccessibilitySize { return 3 }
        return 1
    }

    /// How many lines the thread's own subject may take, at the top of
    /// the thread screen.
    ///
    /// Three is enough for the long subjects mail actually carries, and
    /// bounded so the messages are still on screen when a thread opens.
    /// At the accessibility sizes three lines held four words —
    /// "Quarterly report and the follow-up no…" — and the subject is the
    /// one thing you opened the thread to read.
    static func threadSubjectLines(_ size: DynamicTypeSize) -> Int {
        if size.isAccessibilitySize { return 6 }
        return 3
    }

    /// How many lines the recipient list may take.
    ///
    /// One, middle-truncated, while that shows both ends of an address.
    /// At the accessibility sizes one line held about six characters —
    /// `To: me…ple.com>` — which shows neither.
    static func recipientLines(_ size: DynamicTypeSize) -> Int {
        if size.isAccessibilitySize { return 3 }
        return 1
    }

    /// Where the avatar and the chevron sit against the text.
    ///
    /// Centred while the row is two short lines. Once the subject wraps
    /// to three, centring leaves the face floating beside the middle of
    /// the subject with nothing to do with the name above it — so it
    /// moves to the top, where the name is.
    static func gutterAlignment(_ size: DynamicTypeSize) -> VerticalAlignment {
        if size.isAccessibilitySize { return .top }
        return .center
    }
}
