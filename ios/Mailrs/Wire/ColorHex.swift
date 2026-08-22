import SwiftUI

extension Color {
    /// A colour the server chose, as `#rrggbb`.
    ///
    /// The account dot has to be the same colour on all three clients,
    /// so the value is data rather than a local palette lookup. Anything
    /// unreadable falls back to a neutral grey: a row with no dot reads
    /// as a different kind of account, and a crash over a colour would
    /// be worse than either.
    init(hex: String) {
        let cleaned = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard cleaned.count == 6, let v = UInt32(cleaned, radix: 16) else {
            self = Color(red: 0.42, green: 0.45, blue: 0.50)
            return
        }
        self = Color(
            red: Double((v >> 16) & 0xFF) / 255,
            green: Double((v >> 8) & 0xFF) / 255,
            blue: Double(v & 0xFF) / 255
        )
    }
}
