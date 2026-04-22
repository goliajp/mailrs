import Foundation

struct Endpoint {
    var path: String
    var query: [URLQueryItem] = []

    func url(relativeTo base: URL) -> URL? {
        guard var comps = URLComponents(url: base, resolvingAgainstBaseURL: false) else { return nil }
        let cleanPath = path.hasPrefix("/") ? path : "/" + path
        comps.path = (comps.path.hasSuffix("/") ? String(comps.path.dropLast()) : comps.path) + cleanPath
        if !query.isEmpty {
            comps.queryItems = query
        }
        return comps.url
    }
}

extension Array where Element == URLQueryItem {
    static func build(_ pairs: KeyValuePairs<String, String?>) -> [URLQueryItem] {
        pairs.compactMap { k, v in v.map { URLQueryItem(name: k, value: $0) } }
    }
}
