import Foundation

/// The two sender lists, and what each one does.
enum SenderListKind: String, CaseIterable, Identifiable, Sendable {
    case allowed
    case blocked

    var id: Self { self }

    /// The route, spelled out.
    ///
    /// Not `"/api/spam/\(segment)"`: `check-dead-routes.sh` can only
    /// see the literals a file contains, and a path assembled from a
    /// variable makes a route that *is* called look like one nobody
    /// calls — which is the report that gate exists to give.
    var listPath: String {
        switch self {
        case .allowed: return "/api/spam/whitelist"
        case .blocked: return "/api/spam/blacklist"
        }
    }

    var title: String {
        switch self {
        case .allowed: return String(localized: "Always allowed")
        case .blocked: return String(localized: "Always blocked")
        }
    }

    var explanation: String {
        switch self {
        case .allowed:
            // One literal: `String(localized:)` takes a literal key,
            // and a concatenation is an expression the catalog cannot
            // be keyed on.
            return String(localized: "Marking a conversation as not junk adds its sender here. Mail from these addresses skips the spam filter.")
        case .blocked:
            return String(localized: "Mail from these addresses is treated as junk on arrival.")
        }
    }
}
