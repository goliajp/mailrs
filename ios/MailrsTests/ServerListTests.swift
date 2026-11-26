import Foundation
import Testing

@testable import Mailrs

struct ServerAddressTests {
    @Test func oneServerHasOneSpelling() {
        let same = [
            "mail.example.com", "https://mail.example.com", "https://mail.example.com/",
            "  HTTPS://Mail.Example.com//  ",
        ]
        for typed in same {
            #expect(ServerAddress.normalise(typed) == "https://mail.example.com")
        }
    }

    @Test func plainHttpPortsAndPathsAreKept() {
        #expect(ServerAddress.normalise("http://localhost:6039") == "http://localhost:6039")
        #expect(ServerAddress.normalise("192.168.1.20:8443") == "https://192.168.1.20:8443")
        #expect(ServerAddress.normalise("example.com/mail/") == "https://example.com/mail")
        #expect(ServerAddress.normalise("[::1]:8080") == "https://[::1]:8080")
    }

    @Test(arguments: [
        "", "   ", "mail example.com", "ftp://mail.example.com", "https://",
        "https://user@mail.example.com", "mail.example.com?x=1", "mail.example.com#top",
        "mail..example.com", ".example.com", "example.com.", "-example.com",
        "mail.example.com:", "mail.example.com:0", "mail.example.com:70000",
        "mail.example.com:abc", "a:b:c", "[::1", "mail,example.com",
    ])
    func whatIsNotAServerIsRefused(typed: String) {
        #expect(ServerAddress.normalise(typed) == nil)
    }

    @Test func aRowShowsTheHostAndNeverHidesPlainHttp() {
        #expect(ServerAddress.display("https://mail.example.com:8443") == "mail.example.com:8443")
        #expect(ServerAddress.display("http://localhost:6039") == "http://localhost:6039")
        #expect(ServerAddress.host("https://mail.example.com:8443/x") == "mail.example.com")
        #expect(ServerAddress.host("https://[::1]:8080") == "[::1]")
    }
}

struct ServerListTests {
    @Test func aNewListIsTheBuiltInServer() {
        let list = ServerList()
        #expect(list.visible == [ServerList.builtIn])
        #expect(list.preselected == ServerList.builtIn)
        #expect(ServerList.builtIn.name == "GoliaMail")
        #expect(ServerList.builtIn.address == "https://mail.golia.ai")
    }

    @Test func addingKeepsTheNameAndTheNormalisedAddress() throws {
        var list = ServerList()
        let office = try list.add(name: "  Office ", address: "Mail.Example.com/")
        #expect(office.name == "Office")
        #expect(office.address == "https://mail.example.com")
        #expect(list.visible == [ServerList.builtIn, office])
    }

    @Test func anEmptyNameBecomesTheHost() throws {
        var list = ServerList()
        let entry = try list.add(name: "  ", address: "https://mail.example.com:8443")
        #expect(entry.name == "mail.example.com")
    }

    @Test func aBadOrRepeatedAddressIsRefusedAndChangesNothing() throws {
        var list = ServerList()
        try list.add(name: "Office", address: "mail.example.com")
        let before = list
        #expect(throws: ServerList.Problem.address) { try list.add(name: "x", address: "not a server") }
        #expect(throws: ServerList.Problem.duplicate) {
            try list.add(name: "Again", address: "https://MAIL.example.com/")
        }
        #expect(throws: ServerList.Problem.duplicate) { try list.add(name: "Golia", address: "mail.golia.ai") }
        #expect(list == before)
    }

    @Test func editingChangesAnAddedServerOnly() throws {
        var list = ServerList()
        let office = try list.add(name: "Office", address: "mail.example.com")
        let home = try list.add(name: "Home", address: "home.example.com")

        try list.edit(id: office.id, name: "Work", address: "mx.example.com")
        #expect(list.visible[1] == SavedServer(id: office.id, name: "Work", address: "https://mx.example.com"))

        // its own address is not a duplicate of itself; another entry's is
        try list.edit(id: office.id, name: "Work 2", address: "mx.example.com")
        #expect(throws: ServerList.Problem.duplicate) {
            try list.edit(id: office.id, name: "Work", address: home.address)
        }
        #expect(throws: ServerList.Problem.address) { try list.edit(id: office.id, name: "Work", address: "") }

        let before = list
        try list.edit(id: ServerList.builtIn.id, name: "Mine", address: "evil.example.com")
        #expect(list == before)
    }

    @Test func deletingRemovesAnAddedServer() throws {
        var list = ServerList()
        let office = try list.add(name: "Office", address: "mail.example.com")
        list.recordSuccess(address: office.address)
        list.remove(id: office.id)
        #expect(list.visible == [ServerList.builtIn])
        #expect(list.lastSuccess == nil)
        #expect(list.preselected == ServerList.builtIn)
    }

    @Test func theBuiltInServerHidesAndComesBack() throws {
        var list = ServerList()
        // alone, it stays: the list is never empty
        #expect(!list.canHideBuiltIn)
        list.remove(id: ServerList.builtIn.id)
        #expect(list.visible == [ServerList.builtIn])

        let office = try list.add(name: "Office", address: "mail.example.com")
        #expect(list.canHideBuiltIn)
        list.remove(id: ServerList.builtIn.id)
        #expect(list.visible == [office])
        #expect(list.canRestoreBuiltIn)

        list.restoreBuiltIn()
        #expect(list.visible == [ServerList.builtIn, office])
        #expect(!list.canRestoreBuiltIn)
    }

    @Test func theHiddenBuiltInReturnsRatherThanLeaveTheListEmpty() throws {
        var list = ServerList()
        let office = try list.add(name: "Office", address: "mail.example.com")
        list.remove(id: ServerList.builtIn.id)
        list.remove(id: office.id)
        #expect(list.visible == [ServerList.builtIn])
        #expect(!list.canRestoreBuiltIn)
    }

    @Test func typingTheHiddenBuiltInAddressBringsItBack() throws {
        var list = ServerList()
        try list.add(name: "Office", address: "mail.example.com")
        list.remove(id: ServerList.builtIn.id)
        let entry = try list.add(name: "Golia again", address: "mail.golia.ai")
        #expect(entry == ServerList.builtIn)
        #expect(list.visible.first == ServerList.builtIn)
        #expect(list.visible.count == 2)
    }

    @Test func theLastSuccessfulServerIsPreselected() throws {
        var list = ServerList()
        let office = try list.add(name: "Office", address: "mail.example.com")
        let home = try list.add(name: "Home", address: "home.example.com")
        #expect(list.preselected == ServerList.builtIn)

        list.recordSuccess(address: "https://home.example.com/")
        #expect(list.preselected == home)
        list.recordSuccess(address: office.address)
        #expect(list.preselected == office)
        list.recordSuccess(address: ServerList.builtIn.address)
        #expect(list.preselected == ServerList.builtIn)
        #expect(list.visible.count == 3)
    }

    @Test func aSuccessAtAnUnlistedAddressSavesIt() {
        var list = ServerList()
        list.recordSuccess(address: "https://mail.example.com:8443")
        #expect(list.visible.count == 2)
        #expect(list.preselected.name == "mail.example.com")
        #expect(list.preselected.address == "https://mail.example.com:8443")

        let once = list
        list.recordSuccess(address: "mail.example.com:8443")
        #expect(list == once)
    }

    @Test func aSuccessAtTheHiddenBuiltInShowsItAgain() throws {
        var list = ServerList()
        try list.add(name: "Office", address: "mail.example.com")
        list.remove(id: ServerList.builtIn.id)
        list.recordSuccess(address: ServerList.builtIn.address)
        #expect(list.visible.first == ServerList.builtIn)
        #expect(list.preselected == ServerList.builtIn)
    }

    @Test func hidingThePreselectedServerFallsBackToTheFirstListed() throws {
        var list = ServerList()
        let office = try list.add(name: "Office", address: "mail.example.com")
        list.recordSuccess(address: ServerList.builtIn.address)
        list.remove(id: ServerList.builtIn.id)
        #expect(list.preselected == office)
    }
}

@MainActor
struct ServerStoreTests {
    private func scratch() -> UserDefaults {
        UserDefaults(suiteName: "mailrs-test-\(UUID().uuidString)")!
    }

    @Test func theListSurvivesARelaunch() throws {
        let defaults = scratch()
        let first = ServerStore(defaults: defaults)
        try first.update { try $0.add(name: "Office", address: "mail.example.com") }
        first.update { $0.recordSuccess(address: "mail.example.com") }
        first.update { $0.remove(id: ServerList.builtIn.id) }

        let second = ServerStore(defaults: defaults)
        #expect(second.list == first.list)
        #expect(second.list.preselected.name == "Office")
        #expect(second.list.canRestoreBuiltIn)
    }

    @Test func readingDoesNotWrite() {
        let defaults = scratch()
        let store = ServerStore(defaults: defaults)
        store.update { $0.restoreBuiltIn() }
        #expect(defaults.object(forKey: "mailrs.servers") == nil)
    }

    @Test func aRefusedChangeIsNotWritten() {
        let defaults = scratch()
        let store = ServerStore(defaults: defaults)
        #expect(throws: ServerList.Problem.address) {
            try store.update { try $0.add(name: "x", address: "no server") }
        }
        #expect(store.list == ServerList())
        #expect(defaults.object(forKey: "mailrs.servers") == nil)
    }

    @Test func aLaunchStartsAtTheServerThatLastSignedIn() throws {
        let store = ServerStore(defaults: scratch())
        #expect(store.launchURL(arguments: []).absoluteString == "https://mail.golia.ai")
        try store.update { try $0.add(name: "Office", address: "mail.example.com") }
        #expect(store.launchURL(arguments: []).absoluteString == "https://mail.golia.ai")
        store.update { $0.recordSuccess(address: "mail.example.com") }
        #expect(store.launchURL(arguments: []).absoluteString == "https://mail.example.com")
    }

    @Test func aDrivenLaunchStartsAtItsStubUnlessItKeepsARememberedList() {
        let stub = ["-mailrsBaseURL", "http://localhost:6039"]
        let store = ServerStore(defaults: scratch())
        #expect(store.launchURL(arguments: stub).absoluteString == "http://localhost:6039")
        // keeping a list that remembers nothing still starts at the stub
        #expect(store.launchURL(arguments: stub + ["-mailrsKeepServers"]).absoluteString
            == "http://localhost:6039")
        store.update { $0.recordSuccess(address: "http://127.0.0.1:6039") }
        #expect(store.launchURL(arguments: stub).absoluteString == "http://localhost:6039")
        #expect(store.launchURL(arguments: stub + ["-mailrsKeepServers"]).absoluteString
            == "http://127.0.0.1:6039")
    }
}
