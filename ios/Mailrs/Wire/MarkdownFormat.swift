import Foundation

/// The composer toolbar's bold, italic and link, applied to the text and
/// the selection the way the web composer does (`web/src/lib/markdown-format.ts`):
/// the same marks, so a message reads the same whichever client wrote it.
///
/// Offsets count characters (`String` elements). Pure, so it is tested
/// without a text view.
enum MarkdownFormat {
    enum Kind: CaseIterable {
        case bold
        case italic
        case link
    }

    struct Edit: Equatable {
        let value: String
        let start: Int
        let end: Int
    }

    static func apply(_ value: String, start: Int, end: Int, kind: Kind) -> Edit {
        let lower = max(0, min(start, end, value.count))
        let upper = max(lower, min(max(start, end), value.count))
        switch kind {
        case .bold:
            return wrap(value, lower, upper, mark: "**")
        case .italic:
            return wrap(value, lower, upper, mark: "_")
        case .link:
            return link(value, lower, upper)
        }
    }

    // wrapping an already wrapped selection takes the marks off again
    private static func wrap(_ value: String, _ start: Int, _ end: Int, mark: String) -> Edit {
        let before = String(value.prefix(start))
        let picked = String(value.dropFirst(start).prefix(end - start))
        let after = String(value.dropFirst(end))
        if before.hasSuffix(mark) && after.hasPrefix(mark) {
            return Edit(
                value: String(before.dropLast(mark.count)) + picked + String(after.dropFirst(mark.count)),
                start: start - mark.count,
                end: end - mark.count
            )
        }
        return Edit(
            value: before + mark + picked + mark + after,
            start: start + mark.count,
            end: end + mark.count
        )
    }

    // the link text stays as typed and the url placeholder is selected, ready to paste over
    private static func link(_ value: String, _ start: Int, _ end: Int) -> Edit {
        let picked = String(value.dropFirst(start).prefix(end - start))
        let url = "https://"
        let head = String(value.prefix(start)) + "[" + picked + "]("
        return Edit(
            value: head + url + ")" + String(value.dropFirst(end)),
            start: head.count,
            end: head.count + url.count
        )
    }
}
