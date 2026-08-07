import Foundation

/// Last-known mail, on disk, so a cold launch opens on the mailbox
/// instead of a spinner.
///
/// Strictly a display cache: every successful fetch overwrites it, a
/// missing or unreadable file answers `nil` and the caller fetches as
/// if it never existed. Nothing here is a source of truth — the server
/// is — so corruption is handled by deletion, not repair.
struct MailCache {
    private let directory: URL

    init(directory: URL? = nil) {
        self.directory = directory ?? FileManager.default
            .urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("mail-cache", isDirectory: true)
    }

    /// `-mailrsFreshCache` wipes at boot: UI tests assert empty-start
    /// behaviours (spinners, empty states) that yesterday's rows would
    /// satisfy or contradict at random.
    static func bootstrap() -> MailCache {
        let cache = MailCache()
        if ProcessInfo.processInfo.arguments.contains("-mailrsFreshCache") {
            try? FileManager.default.removeItem(at: cache.directory)
        }
        return cache
    }

    func readConversations(list: String) -> [Wire.Conversation]? {
        read([Wire.Conversation].self, from: "conversations-\(sanitized(list)).json")
    }

    func writeConversations(_ rows: [Wire.Conversation], list: String) {
        write(rows, to: "conversations-\(sanitized(list)).json")
    }

    private func read<T: Decodable>(_ type: T.Type, from name: String) -> T? {
        let url = directory.appendingPathComponent(name)
        guard let data = try? Data(contentsOf: url) else { return nil }
        guard let value = try? JSONDecoder().decode(type, from: data) else {
            // A file that no longer parses is yesterday's schema.
            // Deleting it is the repair.
            try? FileManager.default.removeItem(at: url)
            return nil
        }
        return value
    }

    private func write<T: Encodable>(_ value: T, to name: String) {
        guard let data = try? JSONEncoder().encode(value) else { return }
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try? data.write(to: directory.appendingPathComponent(name), options: .atomic)
    }

    /// List keys come from an enum today, but a filename component that
    /// trusts its input is a path traversal waiting for the input to
    /// stop being an enum.
    private func sanitized(_ key: String) -> String {
        String(key.unicodeScalars.map { CharacterSet.alphanumerics.contains($0) ? Character($0) : "-" })
    }
}
