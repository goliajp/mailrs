import Foundation

/// Why a message was judged as it was, in the reader's language.
///
/// The server stores a reason as a code and the values its sentence
/// names; the words come from `reasons.json`, a byte-identical copy of
/// the server's table (a Rust test compares every copy). The rule is the
/// web client's: the sentence that names the facts when the reason
/// carries every one of them, the general sentence otherwise.
struct ReasonText: Sendable {
    /// The table's three columns.
    enum Lang: String, Sendable {
        case en, ja, zh

        /// The column for a UI locale: Japanese and Chinese have their
        /// own, everything else reads English.
        static func of(_ locale: Locale) -> Lang {
            switch locale.language.languageCode?.identifier {
            case "ja": return .ja
            case "zh": return .zh
            default: return .en
            }
        }
    }

    struct Entry: Decodable, Sendable {
        let en: String
        let ja: String
        let zh: String
        let params: Params?

        struct Params: Decodable, Sendable {
            let en: String
            let ja: String
            let zh: String
        }
    }

    let table: [String: Entry]

    /// The table shipped in the app.
    static let bundled: ReasonText = {
        guard let url = Bundle.main.url(forResource: "reasons", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let table = try? JSONDecoder().decode([String: Entry].self, from: data)
        else { return ReasonText(table: [:]) }
        return ReasonText(table: table)
    }()

    /// The category names a `class` param can carry, as the reader knows them.
    private static let classNames: [String: [Lang: String]] = [
        "inbox": [.en: "Inbox", .ja: "受信トレイ", .zh: "收件箱"],
        "notification": [.en: "Notifications", .ja: "通知", .zh: "通知"],
        "promotion": [.en: "Promotions", .ja: "プロモーション", .zh: "推广"],
    ]

    func has(_ code: String) -> Bool { table[code] != nil }

    /// The sentence for `reason`, or nil when this build has none for
    /// its code: a rule added on the server after this build.
    func text(_ reason: Wire.Reason, _ lang: Lang) -> String? {
        guard let entry = table[reason.code] else { return nil }
        return fill(sentence(entry, lang, reason.params), reason.params, lang)
    }

    /// The sentence, or the code itself when there is none.
    func words(_ reason: Wire.Reason, _ lang: Lang) -> String {
        text(reason, lang) ?? reason.code
    }

    private func sentence(_ entry: Entry, _ lang: Lang, _ params: [String: String]) -> String {
        let general = Self.column(entry.en, entry.ja, entry.zh, lang)
        guard let p = entry.params else { return general }
        let withParams = Self.column(p.en, p.ja, p.zh, lang)
        if Self.placeholders(withParams).allSatisfy({ params[$0] != nil }) { return withParams }
        return general
    }

    private func fill(_ sentence: String, _ params: [String: String], _ lang: Lang) -> String {
        // one pass, so a value that itself reads `{host}` stays as written
        sentence.replacing(/\{(\w+)\}/) { match -> String in
            let name = String(match.output.1)
            guard let value = params[name] else { return String(match.output.0) }
            if name == "class", let named = Self.classNames[value]?[lang] { return named }
            return value
        }
    }

    private static func column(_ en: String, _ ja: String, _ zh: String, _ lang: Lang) -> String {
        switch lang {
        case .en: return en
        case .ja: return ja
        case .zh: return zh
        }
    }

    /// The `{name}` placeholders in a sentence, in order.
    static func placeholders(_ sentence: String) -> [String] {
        sentence.matches(of: /\{(\w+)\}/).map { String($0.output.1) }
    }
}
