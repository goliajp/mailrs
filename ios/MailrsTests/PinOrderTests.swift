import Testing

@testable import Mailrs

@Suite("Pinned first")
struct PinOrderTests {
    private struct Row: Equatable {
        let id: Int
        let pinned: Bool
    }

    @Test("pinned rows lift out of the server's order")
    func lifts() {
        let rows = [
            Row(id: 1, pinned: false), Row(id: 2, pinned: true),
            Row(id: 3, pinned: false), Row(id: 4, pinned: true),
        ]
        #expect(PinOrder.arrange(rows, pinned: \.pinned).map(\.id) == [2, 4, 1, 3])
    }

    /// The server sorted by activity and that order is the answer — a
    /// comparison sort keyed on `pinned` alone may shuffle same-key rows
    /// and the list would reorder itself between refreshes.
    @Test("order within each group is the server's")
    func stable() {
        let rows = (1...20).map { Row(id: $0, pinned: $0 % 5 == 0) }
        let out = PinOrder.arrange(rows, pinned: \.pinned).map(\.id)
        #expect(out.prefix(4) == [5, 10, 15, 20])
        #expect(Array(out.dropFirst(4)) == (1...20).filter { $0 % 5 != 0 })
    }

    @Test("nothing pinned leaves the list untouched")
    func untouched() {
        let rows = (1...5).map { Row(id: $0, pinned: false) }
        #expect(PinOrder.arrange(rows, pinned: \.pinned) == rows)
        #expect(PinOrder.arrange([Row](), pinned: \.pinned).isEmpty)
    }

    @Test("all pinned is also the list untouched")
    func allPinned() {
        let rows = (1...5).map { Row(id: $0, pinned: true) }
        #expect(PinOrder.arrange(rows, pinned: \.pinned) == rows)
    }
}
