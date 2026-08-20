import Foundation

/// One proposed event as an `.ics` file, for handing to a calendar.
///
/// Written to a temporary file rather than shared as text: Calendar
/// opens a file with this extension and does nothing useful with a
/// string of the same bytes.
enum InviteICS {
    /// The file, or nil when it could not be written.
    ///
    /// **The time is written floating** — no zone, no `Z`. RFC 5545
    /// §3.3.5 says a floating time means local to whoever reads it,
    /// which is exactly what "2pm" in a sentence means and exactly what
    /// this side knows. Stamping it UTC would move the appointment by
    /// the reader's own offset.
    static func write(_ s: Wire.DateSuggestion) -> URL? {
        var lines = [
            "BEGIN:VCALENDAR",
            "VERSION:2.0",
            "PRODID:-//mailrs//datefind//EN",
            "BEGIN:VEVENT",
            "UID:\(s.date)-\(abs(s.text.hashValue))@mailrs",
            "SUMMARY:\(escape(s.text))",
        ]
        if let dt = s.datetime {
            let stamp = dt.replacingOccurrences(of: "-", with: "")
                .replacingOccurrences(of: ":", with: "")
            lines.append("DTSTART:" + stamp)
        } else {
            // A day with no hour is a day. Giving it midnight invents a
            // meeting time nobody wrote.
            lines.append("DTSTART;VALUE=DATE:" + s.date.replacingOccurrences(of: "-", with: ""))
        }
        lines.append(contentsOf: ["END:VEVENT", "END:VCALENDAR"])
        let body = lines.joined(separator: "\r\n") + "\r\n"
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("\(s.date).ics")
        do {
            try body.write(to: url, atomically: true, encoding: .utf8)
            return url
        } catch {
            return nil
        }
    }

    static func escape(_ v: String) -> String {
        v.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: ";", with: "\\;")
            .replacingOccurrences(of: ",", with: "\\,")
            .replacingOccurrences(of: "\n", with: "\\n")
    }
}
