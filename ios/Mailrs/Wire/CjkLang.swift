import Foundation

/// The CJK language a mail body is written in, and the font to set it in.
///
/// A mail names Latin fonts the phone does not have, so every CJK character
/// falls back. With one Japanese font for all mail, characters that font
/// lacks (simplified forms such as 传) fall back again to another font, and
/// one heading comes out in two weights. Naming the language lets the web
/// view pick the regional font for every glyph. Same rule as the web
/// client's `cjk-fonts.ts`.
enum CjkLang: String {
    case ja
    case ko
    case zhHans = "zh-Hans"
    case zhHant = "zh-Hant"

    private static let simplified = Set("这们说时会对国个来为后发经过还动问东车开关见长现体实点电话书学门马让给气样从认题应头将")
    private static let traditional = Set("這們說時會對國個來為後發經過還動問東車開關見長現體實點電話書學門馬讓給氣樣從認題應頭將")

    /// `nil` when the body has no CJK text.
    static func of(_ text: String) -> CjkLang? {
        var han = false
        var simplifiedCount = 0
        var traditionalCount = 0
        for scalar in text.unicodeScalars {
            switch scalar.value {
            case 0x3040...0x30FF: return .ja
            case 0xAC00...0xD7AF: return .ko
            case 0x4E00...0x9FFF: han = true
            default: break
            }
        }
        guard han else { return nil }
        for ch in text {
            if simplified.contains(ch) { simplifiedCount += 1 }
            if traditional.contains(ch) { traditionalCount += 1 }
        }
        if traditionalCount > simplifiedCount { return .zhHant }
        return .zhHans
    }

    /// The system CJK font for this language.
    var font: String {
        switch self {
        case .ja: return "'Hiragino Sans'"
        case .ko: return "'Apple SD Gothic Neo'"
        case .zhHans: return "'PingFang SC'"
        case .zhHant: return "'PingFang TC'"
        }
    }
}
