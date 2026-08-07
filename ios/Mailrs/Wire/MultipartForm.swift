import Foundation

/// multipart/form-data, byte for byte.
///
/// URLSession does not build form bodies, and the encoding is exactly
/// the kind of thing that fails invisibly — a missing CRLF or an
/// unquoted filename parses as an empty field server-side, not an
/// error. So the layout lives in one pure function whose output is
/// unit-tested against the bytes the RFC prescribes.
enum MultipartForm {
    struct FilePart: Equatable {
        let name: String
        let filename: String
        let contentType: String
        let data: Data
    }

    static func encode(
        fields: [(name: String, value: String)],
        files: [FilePart],
        boundary: String
    ) -> Data {
        var out = Data()
        func append(_ text: String) { out.append(Data(text.utf8)) }
        for field in fields {
            append("--\(boundary)\r\n")
            append("Content-Disposition: form-data; name=\"\(field.name)\"\r\n\r\n")
            append("\(field.value)\r\n")
        }
        for file in files {
            append("--\(boundary)\r\n")
            append("Content-Disposition: form-data; name=\"\(file.name)\"; filename=\"\(sanitized(file.filename))\"\r\n")
            append("Content-Type: \(file.contentType)\r\n\r\n")
            out.append(file.data)
            append("\r\n")
        }
        append("--\(boundary)--\r\n")
        return out
    }

    /// A filename is attacker-adjacent input inside a quoted-string:
    /// a quote or CRLF in it would splice the part header.
    private static func sanitized(_ filename: String) -> String {
        filename
            .replacingOccurrences(of: "\"", with: "'")
            .replacingOccurrences(of: "\r", with: " ")
            .replacingOccurrences(of: "\n", with: " ")
    }
}
