import Foundation

/// One server the reader can sign in to: a name they chose and where it is.
struct SavedServer: Codable, Equatable, Identifiable, Sendable {
    var id: String
    var name: String
    /// Always in `ServerAddress.normalise` form.
    var address: String
}

/// The servers offered on the sign-in screen, and which one worked last.
///
/// A value with no storage of its own, so every rule here can be tested
/// without a phone: `ServerStore` is the part that reads and writes it.
struct ServerList: Codable, Equatable, Sendable {
    /// Shipped with the app. It cannot be edited or deleted, only hidden,
    /// and it comes back by itself when it would otherwise leave the list
    /// empty: a sign-in screen with nowhere to sign in to is a dead end.
    static let builtIn = SavedServer(
        id: "builtin.goliamail", name: "GoliaMail", address: "https://mail.golia.ai")

    enum Problem: Error, Equatable {
        /// What was typed is not a server address.
        case address
        /// Another entry already points there.
        case duplicate
    }

    private(set) var added: [SavedServer] = []
    private(set) var builtInHidden = false
    /// The entry of the last successful sign-in, by id.
    private(set) var lastSuccess: String?

    /// What the list shows, built-in first.
    var visible: [SavedServer] {
        if builtInHidden && !added.isEmpty { return added }
        return [Self.builtIn] + added
    }

    /// The built-in entry is hidden and can be brought back.
    var canRestoreBuiltIn: Bool { builtInHidden && !added.isEmpty }

    /// The built-in entry can be hidden: something else would remain.
    var canHideBuiltIn: Bool { !builtInHidden && !added.isEmpty }

    /// Where the sign-in screen starts: the server that last signed in,
    /// or the first one listed.
    var preselected: SavedServer {
        let shown = visible
        return shown.first { $0.id == lastSuccess } ?? shown[0]
    }

    func entry(at address: String) -> SavedServer? {
        guard let wanted = ServerAddress.normalise(address) else { return nil }
        return visible.first { $0.address == wanted }
    }

    /// Add a server. An empty name becomes the host.
    @discardableResult
    mutating func add(name: String, address: String, id: String = UUID().uuidString) throws -> SavedServer {
        // typing the hidden built-in address back in is asking for it back
        if builtInHidden, ServerAddress.normalise(address) == Self.builtIn.address {
            builtInHidden = false
            return Self.builtIn
        }
        let checked = try check(address, except: nil)
        let entry = SavedServer(id: id, name: Self.name(name, for: checked), address: checked)
        added.append(entry)
        return entry
    }

    /// Change a server the reader added. The built-in one is not theirs to change.
    mutating func edit(id: String, name: String, address: String) throws {
        guard let index = added.firstIndex(where: { $0.id == id }) else { return }
        let checked = try check(address, except: id)
        added[index].name = Self.name(name, for: checked)
        added[index].address = checked
    }

    /// Delete an added server, or hide the built-in one.
    mutating func remove(id: String) {
        if id == Self.builtIn.id {
            guard canHideBuiltIn else { return }
            builtInHidden = true
        } else {
            added.removeAll { $0.id == id }
            // nothing left to show instead of it
            if added.isEmpty { builtInHidden = false }
        }
        if lastSuccess == id { lastSuccess = nil }
    }

    mutating func restoreBuiltIn() {
        builtInHidden = false
    }

    /// A sign-in to `address` succeeded: remember it for next time, and
    /// keep it if it was not in the list.
    mutating func recordSuccess(address: String) {
        guard let wanted = ServerAddress.normalise(address) else { return }
        if wanted == Self.builtIn.address {
            builtInHidden = false
            lastSuccess = Self.builtIn.id
        } else if let known = added.first(where: { $0.address == wanted }) {
            lastSuccess = known.id
        } else {
            let entry = SavedServer(
                id: UUID().uuidString, name: ServerAddress.host(wanted), address: wanted)
            added.append(entry)
            lastSuccess = entry.id
        }
    }

    private func check(_ address: String, except id: String?) throws -> String {
        guard let checked = ServerAddress.normalise(address) else { throw Problem.address }
        let others = ([Self.builtIn] + added).filter { $0.id != id }
        if others.contains(where: { $0.address == checked }) { throw Problem.duplicate }
        return checked
    }

    private static func name(_ typed: String, for address: String) -> String {
        let trimmed = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return ServerAddress.host(address) }
        return trimmed
    }
}
