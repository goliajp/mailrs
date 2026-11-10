import SwiftUI

/// Over the Inbox: the whole Inbox or one of its sections, and in New a
/// way to accept the senders shown. The same choice as the web's section
/// bar, on a phone's width.
struct InboxSectionBar: View {
    @Environment(Session.self) private var session
    @Environment(\.theme) private var theme

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                chip("All", section: nil, id: "section.all")
                ForEach(InboxSection.allCases) { section in
                    chip(section.label, section: section, id: "section.\(section.rawValue)")
                }
                if session.inboxSection == .new, !session.conversations.isEmpty {
                    Button("Accept all") { Task { await session.acceptShownSenders() } }
                        .font(.footnote)
                        .accessibilityIdentifier("section.acceptAll")
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
        }
        .background(.bar)
    }

    /// The web's section chips, in the gds neutrals: the chosen one on
    /// `border-strong` in the full text colour and semibold, the others
    /// on `bg-secondary` in the muted colour. An accent wash read as a
    /// navy pill in dark mode.
    private func chip(_ title: LocalizedStringKey, section: InboxSection?, id: String) -> some View {
        let chosen = session.inboxSection == section
        return Button { Task { await session.selectSection(section) } } label: {
            Text(title)
                .font(.footnote.weight(Self.weight(chosen)))
                .foregroundStyle(Self.text(chosen, theme))
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(Capsule().fill(Self.fill(chosen, theme)))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(id)
        .accessibilityAddTraits(Self.traits(chosen))
    }

    private static func fill(_ chosen: Bool, _ theme: Theme) -> Color {
        if chosen { return theme.borderStrong }
        return theme.bgSecondary
    }

    private static func text(_ chosen: Bool, _ theme: Theme) -> Color {
        if chosen { return theme.fg }
        return theme.fgMuted
    }

    private static func weight(_ chosen: Bool) -> Font.Weight {
        if chosen { return .semibold }
        return .regular
    }

    private static func traits(_ chosen: Bool) -> AccessibilityTraits {
        if chosen { return .isSelected }
        return []
    }
}
