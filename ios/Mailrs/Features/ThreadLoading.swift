import SwiftUI

/// The thread while its messages are out: the real header, which the row
/// already knows, over a grey card where the message will be.
///
/// Laid out as the loaded thread is, so opening one (or stepping to the
/// next with the arrows) changes what is in the frames rather than
/// moving them. A spinner in the middle of the screen was replaced by a
/// header and cards at the top.
struct ThreadLoading: View {
    let conversation: Wire.Conversation
    let myAddress: String

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ThreadHeader(conversation: conversation, messages: [], myAddress: myAddress)
                PlaceholderCard()
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
        }
        .scrollDisabled(true)
        .background(Color.groupedBackground)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("thread.loading")
    }
}

private struct PlaceholderCard: View {
    private func bar(_ length: Int) -> String {
        String(repeating: "m", count: length)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 8) {
                Circle()
                    .fill(Color(.tertiarySystemFill))
                    .frame(width: 32, height: 32)
                VStack(alignment: .leading, spacing: 3) {
                    Text(verbatim: bar(9)).font(.subheadline)
                    Text(verbatim: bar(14)).font(.caption)
                }
            }
            VStack(alignment: .leading, spacing: 6) {
                ForEach([22, 24, 20, 23, 12], id: \.self) { length in
                    Text(verbatim: bar(length)).font(.callout)
                }
            }
        }
        .lineLimit(1)
        .redacted(reason: .placeholder)
        .accessibilityHidden(true)
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 12))
    }
}
