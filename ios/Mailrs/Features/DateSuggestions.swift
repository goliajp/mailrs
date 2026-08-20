import SwiftUI

/// The dates somebody wrote in the body, offered as events.
///
/// Most mail about a meeting is not an invitation — no calendar part,
/// no UID, nothing to accept, just a sentence with a time in it. A
/// client without this makes the reader retype what is already on the
/// screen.
///
/// **It offers; it does not file.** The `.ics` goes to the share sheet,
/// where Calendar can take it — no calendar permission is asked for,
/// because nothing is written on the reader's behalf.
struct DateSuggestions: View {
    let suggestions: [Wire.DateSuggestion]
    @Environment(\.theme) private var theme

    var body: some View {
        if !suggestions.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Text("Add to calendar")
                    .font(.caption2)
                    .foregroundStyle(theme.fgMuted)
                ForEach(suggestions) { s in
                    if let file = InviteICS.write(s) {
                        ShareLink(item: file) {
                            Label(s.text, systemImage: "calendar.badge.plus")
                                .font(.caption)
                                .lineLimit(1)
                        }
                        .accessibilityIdentifier("suggestion.\(s.date)")
                    }
                }
            }
            .padding(.top, 6)
        }
    }
}
