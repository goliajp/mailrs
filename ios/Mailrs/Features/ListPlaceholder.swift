import SwiftUI

/// Grey rows standing in for a list whose first page is still out.
///
/// A plain `List` shaped like the real one, not a spinner in the middle:
/// the screen keeps its layout from the switch to the answer, so nothing
/// moves when the rows arrive. A centred spinner put the only thing on
/// screen in the middle and then replaced it with rows at the top.
struct ListPlaceholder: View {
    @Environment(\.theme) private var theme

    var body: some View {
        List(0..<8, id: \.self) { index in
            PlaceholderRow(index: index)
                .listRowBackground(Color.pageBackground)
                .listRowSeparator(ListEdges.top(index == 0), edges: .top)
                .listRowSeparatorTint(theme.borderStrong)
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Color.pageBackground)
        .scrollDisabled(true)
        .allowsHitTesting(false)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading")
        .accessibilityIdentifier("list.loading")
    }
}

/// Which row separators a list draws at its ends: a line above the
/// first row or below the last one has nothing on its other side and
/// reads as a list cut off rather than one that starts or ends.
enum ListEdges {
    static func top(_ isFirst: Bool) -> Visibility {
        if isFirst { return .hidden }
        return .automatic
    }

    static func bottom(_ isLast: Bool) -> Visibility {
        if isLast { return .hidden }
        return .automatic
    }
}

private struct PlaceholderRow: View {
    let index: Int

    /// How long the subject bar is, varied a little row to row so the
    /// block reads as a list of different mail rather than one bar
    /// repeated. The text is never shown: `redacted` draws a bar over
    /// it, which is what makes each bar exactly one line of that font.
    private var subjectLength: Int {
        let lengths = [20, 14, 18, 10]
        return lengths[index % lengths.count]
    }

    private func bar(_ length: Int) -> String {
        String(repeating: "m", count: length)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Circle()
                .fill(Color(.tertiarySystemFill))
                .frame(width: 36, height: 36)
            VStack(alignment: .leading, spacing: 1) {
                HStack {
                    Text(verbatim: bar(7))
                        .font(.subheadline)
                    Spacer(minLength: 4)
                    Text(verbatim: bar(3))
                        .font(.caption)
                }
                Text(verbatim: bar(subjectLength))
                    .font(.subheadline)
                    .lineLimit(1)
                Text(verbatim: bar(16))
                    .font(.caption)
                    .lineLimit(1)
            }
            .alignmentGuide(.listRowSeparatorLeading) { $0[.leading] }
        }
        .padding(.vertical, 2)
        .redacted(reason: .placeholder)
    }
}
