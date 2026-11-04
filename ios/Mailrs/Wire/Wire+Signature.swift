import Foundation

extension Wire {
    /// One stored signature. The server keeps several per user and
    /// marks one default; this client edits the default and leaves the
    /// rest alone, because a phone is not where anyone maintains a set
    /// of them.
    struct Signature: Decodable, Identifiable, Sendable {
        let id: Int64
        let name: String
        let textContent: String
        let isDefault: Bool

        enum CodingKeys: String, CodingKey {
            case id
            case name
            case textContent = "text_content"
            case isDefault = "is_default"
        }
    }

    struct SaveSignatureRequest: Encodable {
        let name: String
        let textContent: String
        let isDefault: Bool

        enum CodingKeys: String, CodingKey {
            case name
            case textContent = "text_content"
            case isDefault = "is_default"
        }
    }

    struct SaveSignatureResponse: Decodable, Sendable {
        let id: Int64
    }
}
