import Foundation

enum JSONCoders {
    static let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.dateDecodingStrategy = .custom { decoder in
            let c = try decoder.singleValueContainer()
            if let seconds = try? c.decode(Double.self) {
                return Date(timeIntervalSince1970: seconds)
            }
            if let seconds = try? c.decode(Int.self) {
                return Date(timeIntervalSince1970: TimeInterval(seconds))
            }
            if let iso = try? c.decode(String.self),
               let date = ISO8601DateFormatter().date(from: iso) {
                return date
            }
            throw DecodingError.dataCorruptedError(
                in: c,
                debugDescription: "Expected unix seconds (number) or ISO8601 string"
            )
        }
        return d
    }()

    static let encoder: JSONEncoder = {
        let e = JSONEncoder()
        e.dateEncodingStrategy = .custom { date, encoder in
            var c = encoder.singleValueContainer()
            try c.encode(date.timeIntervalSince1970)
        }
        return e
    }()
}
