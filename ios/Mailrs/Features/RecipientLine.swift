import SwiftUI

/// The `To:` line of an open message. A long list starts as the reader,
/// a few names and a count (`RecipientSummary`); the count is a button
/// that shows the whole list, which is not laid out until then.
struct RecipientLine: View {
    let recipients: String
    let myAddress: String
    @State private var expanded = false
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        if !expanded, let summary = RecipientSummary.of(recipients, myAddress: myAddress) {
            collapsed(summary)
        } else {
            whole
        }
    }

    // Middle truncation keeps both ends of one address readable, which
    // is the right trade on one line. At the accessibility sizes one line
    // holds about six characters and the result was "me…ple.com>" — so
    // there, it wraps instead. Once expanded, every line it needs.
    private var whole: some View {
        Text("To: \(recipients)")
            .font(.caption2)
            .foregroundStyle(.secondary)
            .lineLimit(lineLimit)
            .truncationMode(.middle)
    }

    private var lineLimit: Int? {
        if expanded { return nil }
        return RowLayout.recipientLines(typeSize)
    }

    private func collapsed(_ summary: RecipientSummary) -> some View {
        HStack(spacing: 4) {
            Text("To: \(names(summary))")
                .lineLimit(RowLayout.recipientLines(typeSize))
                .truncationMode(.tail)
            Button {
                expanded = true
            } label: {
                Text(othersLabel(summary.rest))
                    .underline(pattern: .dot)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Show all \(summary.total) recipients")
            .accessibilityIdentifier("recipients-expand")
            .layoutPriority(1)
        }
        .font(.caption2)
        .foregroundStyle(.secondary)
    }

    private func names(_ summary: RecipientSummary) -> String {
        var names = summary.named
        if summary.me { names.insert(AppText.string("Me"), at: 0) }
        return names.joined(separator: ", ")
    }

    /// Two keys rather than one with a plural rule, as elsewhere in the
    /// catalog: a language that counts differently can say so.
    private func othersLabel(_ n: Int) -> LocalizedStringKey {
        if n == 1 { return "and 1 other" }
        return "and \(n) others"
    }
}
