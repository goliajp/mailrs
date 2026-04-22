import Foundation
import Network

/// Thin wrapper around NWPathMonitor exposing current online state and
/// an AsyncStream of changes.
actor NetworkMonitor {
    private let monitor = NWPathMonitor()
    private let queue = DispatchQueue(label: "jp.golia.mailrs.network")

    private var continuations: [UUID: AsyncStream<Bool>.Continuation] = [:]
    private(set) var isOnline: Bool = true

    static let shared = NetworkMonitor()

    init() {
        monitor.pathUpdateHandler = { [weak self] path in
            let online = path.status == .satisfied
            Task { await self?.update(online: online) }
        }
        monitor.start(queue: queue)
    }

    private func update(online: Bool) {
        guard online != isOnline else { return }
        isOnline = online
        for (_, cont) in continuations {
            cont.yield(online)
        }
    }

    func changes() -> AsyncStream<Bool> {
        AsyncStream { continuation in
            let id = UUID()
            continuation.yield(isOnline)
            continuations[id] = continuation
            continuation.onTermination = { [weak self] _ in
                Task { await self?.removeContinuation(id) }
            }
        }
    }

    private func removeContinuation(_ id: UUID) {
        continuations.removeValue(forKey: id)
    }
}
