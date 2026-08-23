import Testing

@testable import Mailrs

/// What to fetch from a POP3 mailbox, and what to remember.
@Suite struct POP3PlanTests {
    private func server(_ pairs: [(Int, String)]) -> [POP3.Uidl] {
        pairs.map { POP3.Uidl(number: $0.0, id: $0.1) }
    }

    /// Nothing seen yet: everything is new.
    @Test func aFirstPassFetchesEverything() {
        let plan = POP3Plan.decide(server: server([(1, "a"), (2, "b")]), seen: [])
        #expect(plan.fetch == [1, 2])
        #expect(plan.deferred == 0)
    }

    @Test func whatHasBeenSeenIsNotFetchedAgain() {
        let plan = POP3Plan.decide(server: server([(1, "a"), (2, "b")]), seen: ["a"])
        #expect(plan.fetch == [2])
        #expect(plan.keep.contains("a"))
    }

    /// The numbers are renumbered every session, so the same message
    /// can be 3 today and 1 tomorrow. Only the uidl decides.
    @Test func identityIsTheUidlAndNeverTheNumber() {
        let plan = POP3Plan.decide(server: server([(1, "b"), (2, "c")]), seen: ["a", "b"])
        #expect(plan.fetch == [2])
        // "a" is gone from the server, so it goes from the set too.
        #expect(plan.keep == ["b"])
    }

    /// A first sync of a mailbox with thousands of messages must not
    /// download all of them before anything appears on screen — and
    /// the newest are the ones somebody is looking for.
    @Test func aLargeMailboxIsFetchedNewestFirstAndBounded() {
        let all = (1...500).map { POP3.Uidl(number: $0, id: "id\($0)") }
        let plan = POP3Plan.decide(server: all, seen: [], limit: 100)
        #expect(plan.fetch.count == 100)
        #expect(plan.deferred == 400)
        #expect(plan.fetch == Array(401...500))
    }

    /// The set is pruned to what the server still has, or a year of
    /// bookkeeping outgrows the mailbox it is about.
    @Test func idsThatHaveGoneFromTheServerGoFromTheSet() {
        let plan = POP3Plan.decide(
            server: server([(5, "e")]), seen: ["a", "b", "c", "d", "e"])
        #expect(plan.keep == ["e"])
        #expect(plan.fetch.isEmpty)
    }

    /// An empty mailbox is not an error and not a crash.
    @Test func anEmptyMailboxAsksForNothing() {
        let plan = POP3Plan.decide(server: [], seen: ["a"])
        #expect(plan.fetch.isEmpty)
        #expect(plan.keep.isEmpty)
        #expect(plan.deferred == 0)
    }
}
