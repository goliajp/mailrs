import Foundation
import Testing

@testable import Mailrs

/// The catalog against `wording.json`, the wording table the web reads and
/// the app ships a byte-identical copy of (`scripts/check-wording.sh`
/// compares the copies and the catalog source). This reads what the built
/// app resolves in each language, so a catalog entry the build dropped or
/// overrode shows here too.
struct WordingTableTests {
    private struct Term: Decodable {
        let id: String
        let text: [String: String]
        let ios: [String]
    }

    private struct Table: Decodable { let terms: [Term] }

    /// The table's language code and the folder the app keeps it in.
    private static let languages = [("en", "en"), ("ja", "ja"), ("zh", "zh-Hans")]

    @Test func everyKeySaysWhatTheTableSays() throws {
        let app = Bundle(for: Session.self)
        let url = try #require(app.url(forResource: "wording", withExtension: "json"),
                               "the app does not ship wording.json")
        let terms = try JSONDecoder().decode(Table.self, from: Data(contentsOf: url)).terms
        #expect(!terms.isEmpty)
        for (lang, folder) in Self.languages {
            let path = try #require(app.path(forResource: folder, ofType: "lproj"),
                                    "the app shipped no \(folder) localization")
            let bundle = try #require(Bundle(path: path))
            for term in terms {
                // the table writes the web's `{name}`; the catalog, `%@`
                let want = try #require(term.text[lang])
                    .replacingOccurrences(of: #"\{\w+\}"#, with: "%@", options: .regularExpression)
                for key in term.ios {
                    #expect(bundle.localizedString(forKey: key, value: key, table: nil) == want,
                            "\(term.id): \(key) in \(lang)")
                }
            }
        }
    }
}
