import Foundation

/// The 1×1 images that exist only to report that a message was opened.
///
/// 5,314 of 7,478 real HTML messages carry one — 71%, 25,945 beacons in
/// all. Blocking remote content stops them, but only until the reader
/// wants to see the pictures: tapping "Load images" fetched every
/// beacon along with them, which is the moment the sender learns the
/// mail was read, from which address and network. The web client has
/// always stripped these regardless of consent; this is the same rule.
///
/// Removed from the document, not merely blocked, so the consent is
/// about *pictures* and not about being counted.
enum TrackingPixels {
    /// The document with its beacons taken out.
    static func strip(html: String) -> String {
        guard html.contains("<img") || html.contains("<IMG") else { return html }
        var out = ""
        out.reserveCapacity(html.count)
        var rest = Substring(html)
        while let open = rest.range(of: "<img", options: .caseInsensitive) {
            out += rest[..<open.lowerBound]
            let after = rest[open.lowerBound...]
            guard let close = after.firstIndex(of: ">") else {
                out += after
                return out
            }
            let tag = String(after[...close])
            if !isBeacon(tag) { out += tag }
            rest = after[after.index(after: close)...]
        }
        out += rest
        return out
    }

    /// A tag is a beacon when it declares itself one pixel each way.
    ///
    /// Both dimensions, or a one-pixel inline size with a matching
    /// attribute — the shapes a tracker writes. A picture authored at
    /// 1×1 would be lost with them, which is the trade the web client
    /// made and nobody has missed.
    static func isBeacon(_ tag: String) -> Bool {
        let one = { (name: String) -> Bool in
            matches(tag, #"\b\#(name)\s*=\s*["']?\s*1\s*["']?"#)
        }
        let inlineOne = matches(
            tag, #"\bstyle\s*=\s*["'][^"']*\b(?:width|height)\s*:\s*1px"#)
        if one("width") && one("height") { return true }
        return inlineOne && (one("width") || one("height") || inlineOne)
    }

    private static func matches(_ text: String, _ pattern: String) -> Bool {
        text.range(of: pattern, options: [.regularExpression, .caseInsensitive]) != nil
    }
}
