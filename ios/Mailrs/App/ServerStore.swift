import Foundation
import Observation

/// Reads a `-flag value` pair off the launch arguments.
///
/// A free function so stored-property initialisers can call it.
func launchValue(
    _ flag: String, in arguments: [String] = ProcessInfo.processInfo.arguments
) -> String? {
    guard let index = arguments.firstIndex(of: flag), index + 1 < arguments.count else {
        return nil
    }
    return arguments[index + 1]
}

/// The saved servers, kept between launches.
///
/// `UserDefaults`, not the Keychain: an address is not a credential, and
/// it has to outlive a sign-out, which clears the Keychain's half.
@Observable
@MainActor
final class ServerStore {
    static let shared = ServerStore.forLaunch(arguments: ProcessInfo.processInfo.arguments)

    private(set) var list: ServerList
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        // absent on a first launch, and then the list is the built-in one
        list = defaults.data(forKey: Self.key)
            .flatMap { try? JSONDecoder().decode(ServerList.self, from: $0) } ?? ServerList()
    }

    /// Change the list and write it, unless nothing changed.
    func update(_ change: (inout ServerList) throws -> Void) rethrows {
        var next = list
        try change(&next)
        guard next != list else { return }
        list = next
        defaults.set(try? JSONEncoder().encode(next), forKey: Self.key)
    }

    /// The store a launch uses.
    ///
    /// A run a UI test is driving (told by `-mailrsBaseURL`) keeps its
    /// list apart from the reader's and starts from an empty one, so a
    /// stub address never reaches a real list and one test's servers are
    /// not the next test's. `-mailrsKeepServers` skips the emptying, for
    /// a test about what a relaunch remembers.
    static func forLaunch(arguments: [String]) -> ServerStore {
        guard arguments.contains("-mailrsBaseURL") else { return ServerStore() }
        let suite = "jp.golia.mailrs.servers.driven"
        let defaults = UserDefaults(suiteName: suite) ?? .standard
        if !arguments.contains("-mailrsKeepServers") {
            defaults.removePersistentDomain(forName: suite)
        }
        return ServerStore(defaults: defaults)
    }

    /// Where the app points when it starts: the server that last signed
    /// in. A driven run starts at its stub instead, unless it asked to
    /// keep its list and that list remembers a sign-in.
    func launchURL(arguments: [String] = ProcessInfo.processInfo.arguments) -> URL {
        let builtIn = URL(string: ServerList.builtIn.address)!
        let remembered = URL(string: list.preselected.address) ?? builtIn
        guard let stub = launchValue("-mailrsBaseURL", in: arguments).flatMap(URL.init(string:)) else {
            return remembered
        }
        if arguments.contains("-mailrsKeepServers"), list.lastSuccess != nil { return remembered }
        return stub
    }

    private static let key = "mailrs.servers"
}
