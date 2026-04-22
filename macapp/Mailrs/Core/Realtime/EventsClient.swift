import Foundation

/// Drives a WebSocket connection to `/api/events` and surfaces decoded
/// `MailEvent` values along with a `ConnectionStatus` stream.
actor EventsClient {
    let events: AsyncStream<MailEvent>
    let statuses: AsyncStream<ConnectionStatus>

    private let eventContinuation: AsyncStream<MailEvent>.Continuation
    private let statusContinuation: AsyncStream<ConnectionStatus>.Continuation

    private let wsBaseURL: URL
    private let session: URLSession

    private var task: URLSessionWebSocketTask?
    private var pingTask: Task<Void, Never>?
    private var receiveTask: Task<Void, Never>?
    private var networkTask: Task<Void, Never>?

    private var currentToken: String?
    private var reconnectDelay: TimeInterval = 1.0
    private var closed: Bool = true
    private var suspended: Bool = false
    private(set) var status: ConnectionStatus = .offline

    private static let pingInterval: TimeInterval = 30
    private static let reconnectMax: TimeInterval = 30

    init(wsBaseURL: URL = AppConfig.webSocketBaseURL, session: URLSession = .shared) {
        self.wsBaseURL = wsBaseURL
        self.session = session
        let (es, ec) = AsyncStream.makeStream(of: MailEvent.self, bufferingPolicy: .bufferingNewest(100))
        let (ss, sc) = AsyncStream.makeStream(of: ConnectionStatus.self, bufferingPolicy: .bufferingNewest(10))
        self.events = es
        self.eventContinuation = ec
        self.statuses = ss
        self.statusContinuation = sc
    }

    // MARK: start / stop

    func start(token: String) {
        closed = false
        currentToken = token
        reconnectDelay = 1.0
        startNetworkWatcher()
        Task { await self.connect() }
    }

    func stop() {
        closed = true
        cancelTasks()
        networkTask?.cancel(); networkTask = nil
        task?.cancel(with: .goingAway, reason: nil)
        task = nil
        updateStatus(.offline)
    }

    /// Called when scenePhase goes background on iOS. Disconnects but keeps
    /// `closed == false` so `resume()` reconnects.
    func suspend() {
        guard !suspended else { return }
        suspended = true
        cancelTasks()
        task?.cancel(with: .goingAway, reason: nil)
        task = nil
        updateStatus(.offline)
    }

    func resume() {
        guard suspended else { return }
        suspended = false
        reconnectDelay = 1.0
        Task { await self.connect() }
    }

    // MARK: private

    private func connect() async {
        guard !closed, !suspended else { return }
        guard let token = currentToken else { return }

        cancelTasks()
        updateStatus(.connecting)

        var comps = URLComponents(url: wsBaseURL, resolvingAgainstBaseURL: false)
        comps?.path = "/api/events"
        comps?.queryItems = [URLQueryItem(name: "token", value: token)]
        guard let url = comps?.url else {
            scheduleReconnect()
            return
        }

        let newTask = session.webSocketTask(with: url)
        task = newTask
        newTask.resume()
        updateStatus(.connected)
        reconnectDelay = 1.0
        startPing()
        startReceive()
    }

    private func startPing() {
        pingTask = Task { [weak self] in
            guard let self else { return }
            while !(await self.isTerminated()) {
                try? await Task.sleep(nanoseconds: UInt64(Self.pingInterval * 1_000_000_000))
                if Task.isCancelled { return }
                await self.sendPing()
            }
        }
    }

    private func isTerminated() -> Bool { closed || suspended }

    private func sendPing() async {
        guard let task else { return }
        task.send(.string("ping")) { _ in }
    }

    private func startReceive() {
        receiveTask = Task { [weak self] in
            guard let self else { return }
            while let message = await self.receiveOne() {
                switch message {
                case .string(let raw):
                    if raw == "ping" || raw == "pong" { continue }
                    if let data = raw.data(using: .utf8) {
                        await self.dispatch(data: data)
                    }
                case .data(let data):
                    await self.dispatch(data: data)
                @unknown default:
                    break
                }
                if Task.isCancelled { return }
            }
            await self.handleDisconnect()
        }
    }

    private func receiveOne() async -> URLSessionWebSocketTask.Message? {
        guard let task else { return nil }
        do {
            return try await task.receive()
        } catch {
            return nil
        }
    }

    private func dispatch(data: Data) {
        do {
            let event = try JSONCoders.decoder.decode(MailEvent.self, from: data)
            eventContinuation.yield(event)
        } catch {
            // Silently ignore undecodable events.
        }
    }

    private func handleDisconnect() {
        guard !closed, !suspended else { return }
        updateStatus(.connecting)
        cancelPingAndReceive()
        task = nil
        scheduleReconnect()
    }

    private func scheduleReconnect() {
        let delay = reconnectDelay
        reconnectDelay = min(reconnectDelay * 2, Self.reconnectMax)
        Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
            await self?.connect()
        }
    }

    private func startNetworkWatcher() {
        networkTask?.cancel()
        networkTask = Task { [weak self] in
            guard let self else { return }
            for await online in await NetworkMonitor.shared.changes() {
                if online {
                    await self.onNetworkRestored()
                } else {
                    await self.onNetworkLost()
                }
            }
        }
    }

    private func onNetworkRestored() async {
        guard !closed, !suspended else { return }
        reconnectDelay = 1.0
        await connect()
    }

    private func onNetworkLost() {
        cancelPingAndReceive()
        task?.cancel(with: .goingAway, reason: nil)
        task = nil
        updateStatus(.offline)
    }

    private func cancelTasks() {
        cancelPingAndReceive()
    }

    private func cancelPingAndReceive() {
        pingTask?.cancel(); pingTask = nil
        receiveTask?.cancel(); receiveTask = nil
    }

    private func updateStatus(_ new: ConnectionStatus) {
        guard new != status else { return }
        status = new
        statusContinuation.yield(new)
    }
}
