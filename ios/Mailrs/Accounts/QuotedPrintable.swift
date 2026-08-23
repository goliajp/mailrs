import Foundation

/// RFC 2045 §6.7.
enum QuotedPrintable {
    static func decode(_ input: Data) -> Data {
        var out = Data()
        out.reserveCapacity(input.count)
        var i = input.startIndex
        while i < input.endIndex {
            let byte = input[i]
            guard byte == 0x3D else {  // '='
                out.append(byte)
                i += 1
                continue
            }
            // A soft line break: `=` at end of line means the line was
            // wrapped there and there is no character at all.
            if i + 2 < input.endIndex, input[i + 1] == 0x0D, input[i + 2] == 0x0A {
                i += 3
                continue
            }
            if i + 1 < input.endIndex, input[i + 1] == 0x0A {
                i += 2
                continue
            }
            if i + 2 < input.endIndex, let hi = hex(input[i + 1]), let lo = hex(input[i + 2]) {
                out.append(hi << 4 | lo)
                i += 3
                continue
            }
            // A lone `=` is not valid, and the choice matters: dropping
            // it silently loses a character somebody typed, so it is
            // kept as itself.
            out.append(byte)
            i += 1
        }
        return out
    }

    private static func hex(_ b: UInt8) -> UInt8? {
        switch b {
        case 0x30...0x39: return b - 0x30
        case 0x41...0x46: return b - 0x41 + 10
        case 0x61...0x66: return b - 0x61 + 10
        default: return nil
        }
    }
}

/// Base64 as it arrives in mail: wrapped across lines, and sometimes
/// with characters no encoder should have emitted.
enum Base64Body {
    static func decode(_ input: Data) -> Data {
        // `Data(base64Encoded:)` refuses anything with a newline in it,
        // which is every base64 body ever sent — they are wrapped at 76
        // characters by the standard that defines them.
        let cleaned = input.filter { byte in
            (byte >= 0x41 && byte <= 0x5A) || (byte >= 0x61 && byte <= 0x7A)
                || (byte >= 0x30 && byte <= 0x39) || byte == 0x2B || byte == 0x2F
                || byte == 0x3D
        }
        var text = String(decoding: cleaned, as: UTF8.self)
        // Padding is often missing from the last line. Without this a
        // whole message decodes to nothing rather than to itself.
        let remainder = text.count % 4
        if remainder == 2 { text += "==" }
        if remainder == 3 { text += "=" }
        if remainder == 1 { text = String(text.dropLast()) }
        return Data(base64Encoded: text) ?? Data()
    }
}
