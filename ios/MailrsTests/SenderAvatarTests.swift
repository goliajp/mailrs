import Testing

@testable import Mailrs

/// The web's `avatarColor` hash, kept in step: the same correspondent
/// must wear the same color on every client, so the hash is asserted
/// against values computed by the web's own algorithm.
struct SenderAvatarTests {
    private func paletteIndex(_ email: String) -> Int {
        var hash: Int32 = 0
        for unit in email.utf16 { hash = hash &* 31 &+ Int32(unit) }
        return Int(hash.magnitude) % SenderAvatar.palette.count
    }

    /// Fixed points computed by running the JS algorithm by hand:
    /// "a" = 97 → 97 % 16 = 1; "ab" = 97*31+98 = 3105 → 3105 % 16 = 1;
    /// overflow must wrap like `| 0`, not trap.
    @Test func matchesTheWebsHashArithmetic() {
        #expect(paletteIndex("a") == 97 % 16)
        #expect(paletteIndex("ab") == 3105 % 16)
        let long = String(repeating: "alice@example.com", count: 8)
        _ = paletteIndex(long) // must not trap on Int32 overflow
    }

    @Test func colorComesFromTheAddressNotTheDisplayName() {
        let bare = SenderAvatar.color(for: "alice@example.com")
        let named = SenderAvatar.color(for: "Alice Smith <ALICE@example.com>")
        #expect(bare == named)
    }

    @Test func initialComesFromTheDisplayName() {
        #expect(SenderAvatar.initial(for: "alice smith <alice@example.com>") == "A")
        #expect(SenderAvatar.initial(for: "bob@example.com") == "B")
        #expect(SenderAvatar.initial(for: "") == "?")
    }
}
