import Foundation

struct MultipartBuilder {
    let boundary: String
    private var body = Data()

    init(boundary: String = "----mailrs-\(UUID().uuidString)") {
        self.boundary = boundary
    }

    var contentTypeHeader: String {
        "multipart/form-data; boundary=\(boundary)"
    }

    mutating func appendText(name: String, value: String) {
        appendBoundary()
        append("Content-Disposition: form-data; name=\"\(escape(name))\"\r\n\r\n")
        append(value)
        append("\r\n")
    }

    mutating func appendFile(name: String, filename: String, contentType: String, data: Data) {
        appendBoundary()
        append("Content-Disposition: form-data; name=\"\(escape(name))\"; filename=\"\(escape(filename))\"\r\n")
        append("Content-Type: \(contentType)\r\n\r\n")
        body.append(data)
        append("\r\n")
    }

    mutating func finalize() -> Data {
        append("--\(boundary)--\r\n")
        return body
    }

    private mutating func appendBoundary() {
        append("--\(boundary)\r\n")
    }

    private mutating func append(_ string: String) {
        if let data = string.data(using: .utf8) {
            body.append(data)
        }
    }

    private func escape(_ s: String) -> String {
        s.replacingOccurrences(of: "\"", with: "\\\"")
            .replacingOccurrences(of: "\n", with: " ")
            .replacingOccurrences(of: "\r", with: " ")
    }
}
