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
                HStack {
                    Image(systemName: "person.crop.circle")
                        .foregroundStyle(.secondary)
                    Text(contact)
                        .font(.subheadline)
                        .lineLimit(1)
                }
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
                await update([])
                return
            }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let found = await session.contacts(matching: token)
            guard !Task.isCancelled else { return }
            await update(found)
        }
    }
}
