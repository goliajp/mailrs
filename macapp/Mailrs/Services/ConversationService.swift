import Foundation

struct ConversationListOptions: Sendable, Equatable {
    var limit: Int = 50
    var before: Int64?
    var category: MailCategory?
    var folder: MailFolder = .inbox
    var quickFilter: QuickFilter = .all
    var domains: [String] = []
    var archived: Bool = false
    var section: String?
}

struct ConversationService {
    let api: ApiClient

    func list(_ options: ConversationListOptions) async throws -> [ConversationSummary] {
        var query: [URLQueryItem] = [URLQueryItem(name: "limit", value: String(options.limit))]

        if let before = options.before {
            query.append(URLQueryItem(name: "before", value: String(before)))
        }
        if let category = options.category {
            query.append(URLQueryItem(name: "category", value: category.rawValue))
        }
        if let folderValue = options.folder.queryValue {
            query.append(URLQueryItem(name: "folder", value: folderValue))
        }
        if !options.domains.isEmpty {
            query.append(URLQueryItem(name: "domains", value: options.domains.joined(separator: ",")))
        }
        if options.archived {
            query.append(URLQueryItem(name: "archived", value: "true"))
        }
        switch options.quickFilter {
        case .unread:
            query.append(URLQueryItem(name: "unread", value: "true"))
        case .starred:
            query.append(URLQueryItem(name: "starred", value: "true"))
        case .all, .attachment:
            // Server has no `attachment` filter — we filter client-side in MailModel.
            break
        }
        if let section = options.section {
            query.append(URLQueryItem(name: "section", value: section))
        }

        return try await api.get("/api/conversations", query: query)
    }

    func search(_ q: String, options: ConversationListOptions) async throws -> [ConversationSummary] {
        var query: [URLQueryItem] = [
            URLQueryItem(name: "q", value: q),
            URLQueryItem(name: "limit", value: String(options.limit)),
        ]
        if let category = options.category {
            query.append(URLQueryItem(name: "category", value: category.rawValue))
        }
        if !options.domains.isEmpty {
            query.append(URLQueryItem(name: "domains", value: options.domains.joined(separator: ",")))
        }
        return try await api.get("/api/conversations/search", query: query)
    }
}
