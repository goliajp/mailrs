import Foundation

/// The parts an HTML body references with `<img src="cid:…">`.
///
/// `multipart/related` mail carries its pictures with it and points at
/// them by Content-ID. The content-blocking rule refuses `http(s)` and
/// nothing serves `cid:`, so until now every one of these rendered as a
/// **broken-image box** — and the same bytes appeared a second time as
/// a file to download, which Apple Mail does not do either. One message
/// in this mailbox shows both at once: a broken frame where the logo
/// should be, and `logo.png` listed above it.
///
/// So the parts are fetched and folded into the document as `data:`
/// URIs. That keeps the promise the remote-image block makes — nothing
/// leaves the device — because these bytes arrived with the message.
enum InlineImages {
    /// Which attachments the body points at, by index.
    ///
    /// Matching is on the Content-ID with angle brackets stripped, and
    /// case-insensitive: senders write `<Logo@x>` in the part header and
    /// `cid:logo@x` in the body about as often as they agree.
    static func referenced(html: String, attachments: [Wire.Attachment]) -> [Int] {
        guard !html.isEmpty else { return [] }
        let ids = cids(in: html)
        guard !ids.isEmpty else { return [] }
        return attachments.enumerated().compactMap { index, attachment in
            guard let raw = attachment.contentId else { return nil }
            let id = normalise(raw)
            guard !id.isEmpty, ids.contains(id) else { return nil }
            return index
        }
    }

    /// Every `cid:` target in the document, normalised.
    static func cids(in html: String) -> Set<String> {
        var out: Set<String> = []
        // Deliberately not an HTML parser: a `cid:` reference is a URL
        // in an attribute, and the set of characters that can end one is
        // small and unambiguous.
        var rest = Substring(html)
        while let range = rest.range(of: "cid:", options: .caseInsensitive) {
            let after = rest[range.upperBound...]
            let end = after.firstIndex { character in
                character == "\"" || character == "'" || character == ">"
                    || character == " " || character == ")" || character == "\n"
            } ?? after.endIndex
            let id = normalise(String(after[..<end]))
            if !id.isEmpty { out.insert(id) }
            rest = after[end...]
        }
        return out
    }

    /// `<Logo@x>` and `Logo@x` are the same part; comparison is
    /// lowercased because the halves rarely agree on case.
    static func normalise(_ contentId: String) -> String {
        contentId
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "<>"))
            .lowercased()
    }

    /// Rewrite `cid:` sources to the data the message brought with it.
    ///
    /// Parts that were not fetched are left alone: a broken image is
    /// better than a `src` pointing at nothing, and it is what the
    /// reader would have seen anyway.
    static func inline(html: String, parts: [String: String]) -> String {
        guard !parts.isEmpty else { return html }
        var out = html
        for (id, dataURI) in parts {
            for quote in ["\"", "'"] {
                out = out.replacingOccurrences(
                    of: "\(quote)cid:\(id)\(quote)", with: "\(quote)\(dataURI)\(quote)",
                    options: .caseInsensitive
                )
            }
            // Unquoted attribute, which older senders still write.
            out = out.replacingOccurrences(
                of: "=cid:\(id)", with: "=\(dataURI)", options: .caseInsensitive
            )
        }
        return out
    }

    /// A `data:` URI for one part. The type comes from the message, not
    /// from the file name — a part labelled `image/png` is what the
    /// document expects to be handed.
    static func dataURI(contentType: String, data: Data) -> String {
        let type = contentType.isEmpty ? "application/octet-stream" : contentType
        return "data:\(type);base64,\(data.base64EncodedString())"
    }
}
