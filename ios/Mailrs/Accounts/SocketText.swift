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
        // Foundation's own decoder, not a character-by-character
        // build. The obvious `String(data.map { Character(...) })`
        // allocates one `Character` per byte — sixteen bytes each,
        // plus an array to hold them — so a 25 MB message becomes
        // hundreds of megabytes and several seconds on a phone. It is
        // correct and it is unusable, which is the worst kind of
        // correct.
        //
        // `.isoLatin1` cannot fail: every byte is a code point in it.
        // The fallback exists because the API is optional, not because
        // there is a case where it is taken.
        String(data: data, encoding: .isoLatin1) ?? ""
    }

    /// The exact bytes back.
    static func bytes(_ s: String) -> Data {
        // The same, in the other direction: `data(using:)` is one pass
        // through Foundation rather than a scalar-by-scalar array.
        //
        // The fallback is not decorative here. `data(using:.isoLatin1)`
        // returns nil when the string holds a code point above U+00FF,
        // which cannot come from `latin1(_:)` but can come from a
        // caller who passed ordinary text — and truncating each scalar
        // to its low byte is what the old code did, so the fallback
        // keeps that behaviour rather than losing the data.
        s.data(using: .isoLatin1) ?? Data(s.unicodeScalars.map { UInt8($0.value & 0xFF) })
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
