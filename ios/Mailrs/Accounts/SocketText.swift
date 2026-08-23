import Foundation

/// Getting bytes back out of what a socket reader produced.
///
/// Named for the socket rather than the wire: `Wire` is already the
/// REST layer's shapes, and two files with one name do not build.
///
/// A mail session carries protocol text (ASCII), folder names (usually
/// modified UTF-7, also ASCII) and message bodies (any encoding at all,
/// declared inside the message). Only the last of those can say what it
/// is, so nothing may be decoded until something has read that
/// declaration.
///
/// ISO-8859-1 is what makes that possible: it maps every byte to the
/// code point of the same value, so it is lossless and reversible. UTF-8
/// is neither — an invalid sequence becomes U+FFFD and the bytes are
/// gone.
enum SocketText {
    /// Bytes as text, one code point per byte.
    static func latin1(_ data: Data) -> String {
        String(data.map { Character(Unicode.Scalar($0)) })
    }

    /// The exact bytes back.
    static func bytes(_ s: String) -> Data {
        Data(s.unicodeScalars.map { UInt8($0.value & 0xFF) })
    }

    /// Read back as UTF-8, for the places that really are text.
    ///
    /// Falls back to what was there when the bytes are not valid UTF-8
    /// — a latin-1 folder name should show as latin-1 rather than as
    /// replacement characters.
    static func utf8(_ s: String) -> String {
        let decoded = String(decoding: bytes(s), as: UTF8.self)
        if decoded.contains("\u{FFFD}") { return s }
        return decoded
    }
}
