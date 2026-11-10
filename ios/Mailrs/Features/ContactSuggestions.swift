import SwiftUI

/// The tappable contact rows under a To field, and the debounce that
/// feeds them. One component for every To field — compose and forward
/// must not grow separate autocomplete behaviours.
struct ContactSuggestions: View {
    @Binding var text: String
    @Binding var suggestions: [String]

    var body: some View {
        ForEach(suggestions, id: \.self) { contact in
            Button {
                text = RecipientAutocomplete.completing(text, with: contact)
                suggestions = []
            } label: {
                // a finger-sized row the whole width, not a line of text
                // packed against the next one
                HStack {
                    Image(systemName: "person.crop.circle")
                        .foregroundStyle(.secondary)
                    Text(contact)
                        .font(.subheadline)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                }
                .frame(minHeight: 40)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
    }

    /// Debounced fetch keyed to the in-progress token. 250ms, the same
    /// beat as search; anything shorter puts one request in flight per
    /// keystroke.
    static func schedule(
        replacing task: Task<Void, Never>?,
        for text: String,
        in session: Session,
        update: @escaping @MainActor ([String]) -> Void
    ) -> Task<Void, Never> {
        task?.cancel()
        return Task {
            let token = RecipientAutocomplete.currentToken(of: text)
            guard RecipientAutocomplete.shouldSuggest(for: token) else {
                update([])
                return
            }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let found = await session.contacts(matching: token)
            guard !Task.isCancelled else { return }
            update(found)
        }
    }
}
