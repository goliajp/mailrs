import SwiftUI

/// The plain-text half of `LinkWarnings`: a body drawn as attributed text
/// rather than in a web view gets its mark as a red "!" after the link,
/// and the question is asked through the `openURL` action.
enum LinkWarningMarks {
    /// The warning a tapped link in a plain-text body falls under. Its
    /// text is the address itself, so a warning about the text showing
    /// another host cannot apply.
    static func warning(for url: URL, in warnings: [Wire.LinkWarning]) -> Wire.LinkWarning? {
        LinkWarnings.warning(
            href: url.absoluteString, text: url.absoluteString,
            in: warnings.filter { $0.shown == nil })
    }

    /// `text` with a mark after every link that falls under a warning.
    static func marked(_ text: AttributedString, warnings: [Wire.LinkWarning]) -> AttributedString {
        guard !warnings.isEmpty else { return text }
        var out = AttributedString()
        for run in text.runs {
            out.append(AttributedString(text[run.range]))
            guard let url = run.link, warning(for: url, in: warnings) != nil else { continue }
            var mark = AttributedString(" !")
            mark.foregroundColor = .red
            mark.font = .caption.weight(.bold)
            out.append(mark)
        }
        return out
    }
}
