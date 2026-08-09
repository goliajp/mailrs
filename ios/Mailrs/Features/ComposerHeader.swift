import SwiftUI

/// Which part of a composer has focus. Shared by both composers so the
/// header can move focus into a field it does not own.
enum ComposerField: Hashable {
    case to
    case cc
    case bcc
    case subject
    case body

    var identifier: String {
        switch self {
        case .to: return "composer-to"
        case .cc: return "composer-cc"
        case .bcc: return "composer-bcc"
        case .subject: return "composer-subject"
        case .body: return "composer-body"
        }
    }

    /// An example, not an instruction: a field showing `copy@example.com`
    /// says both what it wants and what shape it wants it in. Distinct
    /// per field so three address rows are three findable things.
    var placeholder: LocalizedStringKey {
        switch self {
        case .to: return "someone@example.com"
        case .cc: return "copy@example.com"
        case .bcc: return "blind@example.com"
        // Nothing for the subject: `label(_:)` below returns the very
        // same word, and the two together read "Subject  Subject" — at
        // every text size, not only the large ones. The addresses keep
        // theirs because `someone@example.com` says something the word
        // "To" does not: the shape it wants.
        case .subject: return ""
        // The body has no label row; this is the only thing naming it.
        case .body: return "Message"
        }
    }

    var isAddress: Bool {
        switch self {
        case .to, .cc, .bcc: return true
        case .subject, .body: return false
        }
    }
}

/// A header row is either typed into or read off — the reply sheet
/// derives its To and Subject from the thread, and a text field the
/// writer cannot change would be a lie about who can change it.
enum ComposerSlot {
    case editable(Binding<String>)
    case fixed(String)
}

/// The address block above a composer: To, Cc, Bcc, Subject.
///
/// A `Grid`, not a stack of `HStack`s. Each row used to size its own
/// label, so "To" and "Subject" pushed their fields to different
/// x-positions and the block read as two ragged columns — and that
/// ragged edge is the one the eye follows down the form. A grid gives
/// every label one column, sized to the longest, which also survives
/// translation: `件名` is not `Subject`, and a hard-coded width that
/// fits one clips the other.
///
/// Cc and Bcc stay folded until asked for. They are the fields most
/// messages do not use, and two empty rows above the subject push the
/// body — the thing being written — down the screen.
struct ComposerHeader: View {
    let to: ComposerSlot
    @Binding var cc: String
    @Binding var bcc: String
    let subject: ComposerSlot
    @Binding var showsCopies: Bool
    /// Suggestions for whichever address field has focus.
    @Binding var suggestions: [String]

    let focus: FocusState<ComposerField?>.Binding
    @Environment(\.dynamicTypeSize) private var typeSize
    /// Called when an address field changes, with its text — the owner
    /// decides whether to ask the server for contacts.
    var onAddressEdit: (String) -> Void = { _ in }

    var body: some View {
        VStack(spacing: 0) {
            Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 0) {
                row(.to, slot: to) {
                    // The disclosure rides on the To row rather than
                    // taking one of its own: a control that only reveals
                    // two fields should not cost a line while they are
                    // hidden.
                    copiesButton
                }
                divider
                suggestions(under: .to)
                if showsCopies {
                    row(.cc, slot: .editable($cc))
                    divider
                    suggestions(under: .cc)
                    row(.bcc, slot: .editable($bcc))
                    divider
                    suggestions(under: .bcc)
                }
                row(.subject, slot: subject)
            }
            Divider()
        }
    }

    @ViewBuilder private var copiesButton: some View {
        if !showsCopies {
            Button {
                showsCopies = true
                focus.wrappedValue = .cc
            } label: {
                Text(verbatim: "Cc")
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(Color(.tertiarySystemFill), in: Capsule())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Add Cc and Bcc")
        }
    }

    private var divider: some View {
        GridRow { Divider().gridCellColumns(2) }
    }

    /// Suggestions hang under the field that produced them, in the
    /// second column, so they line up with the addresses and not with
    /// the labels.
    @ViewBuilder private func suggestions(under field: ComposerField) -> some View {
        if focus.wrappedValue == field, !suggestions.isEmpty {
            GridRow {
                Color.clear.frame(width: 0, height: 0)
                VStack(alignment: .leading, spacing: 0) {
                    ContactSuggestions(text: binding(for: field), suggestions: $suggestions)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            divider
        }
    }

    private func binding(for field: ComposerField) -> Binding<String> {
        switch field {
        case .cc: return $cc
        case .bcc: return $bcc
        default: return editableTo
        }
    }

    /// The To field's binding when there is one. A fixed To cannot take
    /// a suggestion, and `.constant` says so rather than silently
    /// dropping the tap.
    private var editableTo: Binding<String> {
        if case .editable(let text) = to { return text }
        return .constant("")
    }

    private func label(_ field: ComposerField) -> LocalizedStringKey {
        switch field {
        case .to: return "To"
        case .cc: return "Cc"
        case .bcc: return "Bcc"
        case .subject: return "Subject"
        case .body: return "Message"
        }
    }

    @ViewBuilder private func row<Trailing: View>(
        _ field: ComposerField, slot: ComposerSlot,
        @ViewBuilder trailing: () -> Trailing = { EmptyView() }
    ) -> some View {
        GridRow {
            if RowLayout.stacksHeader(typeSize) {
                // One cell across both columns, stacked. Side by side at
                // the accessibility sizes the label took a third of the
                // width and the address field showed "someone@exa…" — a
                // field you cannot read back what you typed into it is
                // not a field.
                VStack(alignment: .leading, spacing: 2) {
                    Text(label(field))
                        .foregroundStyle(.secondary)
                    HStack(spacing: 6) {
                        content(field, slot: slot)
                        trailing()
                    }
                }
                .gridCellColumns(2)
            } else {
                Text(label(field))
                    .foregroundStyle(.secondary)
                    .gridColumnAlignment(.leading)
                HStack(spacing: 6) {
                    content(field, slot: slot)
                    trailing()
                }
            }
        }
        .font(.subheadline)
        .padding(.horizontal, 12)
        .padding(.vertical, 7)
    }

    @ViewBuilder private func content(
        _ field: ComposerField, slot: ComposerSlot
    ) -> some View {
        switch slot {
        case .fixed(let value):
            // Middle-truncated: the domain is what tells two similar
            // addresses apart, and a tail-truncated list of recipients
            // loses exactly that.
            Text(verbatim: value)
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)
        case .editable(let text):
            input(field, text: text)
        }
    }

    @ViewBuilder private func input(
        _ field: ComposerField, text: Binding<String>
    ) -> some View {
        if field.isAddress {
            TextField(field.placeholder, text: text)
                .textContentType(.emailAddress)
                .keyboardType(.emailAddress)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .focused(focus, equals: field)
                .accessibilityIdentifier(field.identifier)
                .onChange(of: text.wrappedValue) { _, value in onAddressEdit(value) }
        } else {
            TextField(field.placeholder, text: text)
                .focused(focus, equals: field)
                .accessibilityIdentifier(field.identifier)
        }
    }
}
