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

/// The pin, in the two states it has.
enum PinLabel {
    static func title(pinned: Bool) -> LocalizedStringKey {
        if pinned { return "Unpin" }
        return "Pin to top"
    }

    static func icon(pinned: Bool) -> String {
        if pinned { return "pin.slash" }
        return "pin"
    }
}

/// A `TextEditor` that says what it is for while it is empty.
///
/// SwiftUI's has no placeholder, so an empty composer is an unlabelled
/// rectangle — the To and Subject lines above it are named and the
/// largest field on the screen is not. The ghost sits behind the
/// editor and takes no touches, so tapping anywhere still lands in the
/// text.
struct ComposerEditor: View {
    @Binding var text: String
    let placeholder: LocalizedStringKey
    /// The bold, italic and link row the web composer has, writing the
    /// same Markdown marks. Off for fields that are not a message.
    var formatting = false

    @State private var selection: TextSelection?

    var body: some View {
        VStack(spacing: 0) {
            if formatting {
                FormatBar { kind in apply(kind) }
            }
            ZStack(alignment: .topLeading) {
                if text.isEmpty {
                    Text(placeholder)
                        .foregroundStyle(.tertiary)
                        .padding(.horizontal, 13)
                        .padding(.vertical, 8)
                        .allowsHitTesting(false)
                }
                TextEditor(text: $text, selection: $selection)
                    // Its own background is opaque and drew over the ghost
                    // behind it — the placeholder was there and invisible.
                    .scrollContentBackground(.hidden)
                    .padding(.horizontal, 8)
            }
        }
    }

    /// The selection as character offsets; no selection is the end of the text.
    ///
    /// Through UTF-16 offsets clamped to the text as it is now: a selection
    /// made before the text was replaced (a template applied) holds indices
    /// into the old string, and measuring the new one with them can trap.
    private var offsets: (Int, Int) {
        guard let selection, case .selection(let range) = selection.indices else {
            return (text.count, text.count)
        }
        let units = Array(text.utf16)
        func characters(_ index: String.Index) -> Int {
            let clamped = min(max(0, index.utf16Offset(in: text)), units.count)
            return String(decoding: units.prefix(clamped), as: UTF16.self).count
        }
        return (characters(range.lowerBound), characters(range.upperBound))
    }

    private func apply(_ kind: MarkdownFormat.Kind) {
        let (start, end) = offsets
        let edit = MarkdownFormat.apply(text, start: start, end: end, kind: kind)
        text = edit.value
        let lower = text.index(text.startIndex, offsetBy: edit.start)
        let upper = text.index(text.startIndex, offsetBy: edit.end)
        selection = TextSelection(range: lower..<upper)
    }
}

/// Bold, italic, link: three buttons over the message body.
struct FormatBar: View {
    let onFormat: (MarkdownFormat.Kind) -> Void

    var body: some View {
        HStack(spacing: 16) {
            Button { onFormat(.bold) } label: { Image(systemName: "bold") }
                .accessibilityLabel("Bold")
                .accessibilityIdentifier("format-bold")
            Button { onFormat(.italic) } label: { Image(systemName: "italic") }
                .accessibilityLabel("Italic")
                .accessibilityIdentifier("format-italic")
            Button { onFormat(.link) } label: { Image(systemName: "link") }
                .accessibilityLabel("Link")
                .accessibilityIdentifier("format-link")
            Spacer(minLength: 0)
        }
        .font(.subheadline)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
    }
}
