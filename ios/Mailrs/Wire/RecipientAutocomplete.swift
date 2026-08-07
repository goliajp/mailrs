import Foundation

/// The token arithmetic under the To field's contact suggestions.
///
/// A To line is comma/semicolon-separated; suggestions apply to the
/// token the cursor is in — in practice the last one, since address
/// entry is append-shaped. Completion replaces that token with the
/// picked contact's addr-spec and reopens the list for the next
/// entry with a trailing separator.
enum RecipientAutocomplete {
    /// What the person is currently typing: the text after the last
    /// separator, trimmed. Empty when they just finished an entry.
    static func currentToken(of text: String) -> String {
        guard let last = text.lastIndex(where: { $0 == "," || $0 == ";" }) else {
            return text.trimmingCharacters(in: .whitespaces)
        }
        return String(text[text.index(after: last)...])
            .trimmingCharacters(in: .whitespaces)
    }

    /// Whether a suggestion query is worth a request: two characters,
    /// same floor as search, and not already a complete address —
    /// suggesting for "alice@example.com" is answering a question that
    /// was already answered.
    static func shouldSuggest(for token: String) -> Bool {
        token.count >= 2 && !token.contains("@")
    }

    /// The text with the in-progress token replaced by the picked
    /// contact's bare address, ready for the next entry.
    static func completing(_ text: String, with contact: String) -> String {
        let email = SenderName.extractEmail(contact)
        guard let last = text.lastIndex(where: { $0 == "," || $0 == ";" }) else {
            return email + ", "
        }
        let kept = String(text[...last])
        return kept + " " + email + ", "
    }
}
