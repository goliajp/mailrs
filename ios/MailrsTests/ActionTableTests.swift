import Foundation
import Testing

@testable import Mailrs

/// `RowAction` against `actions.json`, the action table the web reads and
/// the app ships a byte-identical copy of (`scripts/check-action-parity.sh`
/// compares the copies). The enum stays the app's own, typed, code; this
/// says it has not drifted from the shared table, field by field.
struct ActionTableTests {
    private struct Entry: Decodable {
        let id: String
        let surfaces: [String]
        let undo: String?
        let confirm: Bool
        let verb: String?
        let leavesList: Bool
        let label: [String: String?]
        let done: [String: String?]
        let icon: [String: String?]

        enum CodingKeys: String, CodingKey {
            case id, surfaces, undo, confirm, verb, label, done, icon
            case leavesList = "leaves_list"
        }
    }

    private struct Table: Decodable { let actions: [Entry] }

    private func table() throws -> [String: Entry] {
        let url = try #require(Bundle.main.url(forResource: "actions", withExtension: "json"),
                               "the app does not ship actions.json")
        let decoded = try JSONDecoder().decode(Table.self, from: Data(contentsOf: url))
        return Dictionary(uniqueKeysWithValues: decoded.actions
            .filter { $0.surfaces.contains("ios") }
            .map { ($0.id, $0) })
    }

    /// `deleteForever` is the table's `delete-forever`.
    private func tableId(_ action: RowAction) -> String {
        action.rawValue.reduce(into: "") { out, ch in
            if ch.isUppercase {
                out += "-" + ch.lowercased()
                return
            }
            out.append(ch)
        }
    }

    @Test func offersExactlyTheTablesActions() throws {
        let ids = Set(try table().keys)
        #expect(Set(RowAction.allCases.map(tableId)) == ids)
    }

    @Test func everyActionMatchesItsEntry() throws {
        let entries = try table()
        for action in RowAction.allCases {
            let id = tableId(action)
            let e = try #require(entries[id], "\(id) is not in actions.json")
            #expect(action.confirm == e.confirm, "\(id) confirm")
            #expect(action.undo.map(tableId) == e.undo.flatMap { entries[$0]?.id }, "\(id) undo")
            #expect(action.titleKey == e.label["ios"] ?? nil, "\(id) label")
            #expect(action.doneKey == e.done["ios"] ?? nil, "\(id) done")
            #expect(action.systemImage == e.icon["ios"] ?? nil, "\(id) icon")
            #expect(action.batch == e.verb, "\(id) verb")
            #expect(action.leavesList == e.leavesList, "\(id) leaves the list")
        }
    }
}
