import SwiftUI

/// What the empty search field offers: the reader's saved searches, each
/// filling the field with its query, and the operators the server takes,
/// the same line the web puts in its search box.
struct SavedSearchSuggestions: View {
    @Environment(Session.self) private var session

    var body: some View {
        Section {
            ForEach(session.savedSearches) { saved in
                Label(saved.name, systemImage: "magnifyingglass")
                    .lineLimit(1)
                    .searchCompletion(saved.query)
                    .accessibilityIdentifier("search.saved.\(saved.id)")
            }
            Text("Search, or use from: to: subject: has:attachment in: is: label:")
                .font(.footnote)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("search.operators")
        } header: {
            if !session.savedSearches.isEmpty {
                Text("Saved searches")
            }
        }
        .task { await session.loadSavedSearches() }
    }
}
