import Foundation

extension Session {
    /// Fetched each time the search opens, so one saved on the web since
    /// shows up. A failed fetch keeps what was there and says nothing, as
    /// the web's saved-search bar does: the field still searches.
    func loadSavedSearches() async {
        guard let client else { return }
        guard let list = try? await client.savedSearches() else { return }
        savedSearches = list
    }
}
