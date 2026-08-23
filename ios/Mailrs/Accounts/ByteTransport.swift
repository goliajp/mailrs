import Foundation
import Network

/// Where a mail session's bytes go.
///
/// A seam, for two reasons that turned out to be the same one.
///
/// **Testing.** Every rule a session enforces — a tag that must not be
/// matched by its own prefix, a literal read by the byte count the
/// server announced, a 334 that is a refusal and not a success — is a
/// rule about what arrives. A real server will not send the awkward
/// case on demand; a scripted one sends it every time.
///
/// **STARTTLS.** `NWConnection` cannot add TLS to a connection that is
/// already running, so a session that must upgrade in place needs a
/// different transport underneath it — and having a seam means the
/// conversation above does not change at all.
/// An actor, so the session above it — itself an actor — can hand the
/// transport across isolation without either of them promising
/// thread-safety by hand. The alternative is `@unchecked Sendable`,
/// which is a claim rather than a guarantee.
protocol ByteTransport: Actor {
    func connect() async throws
    func receive() async throws -> Data
    func send(_ data: Data) async throws
    /// Encrypt an already-open connection. Throws where that cannot be
    /// done, which is how a caller finds out rather than assuming.
    func upgradeToTLS() async throws
    func close()
}

enum TransportFailure: Error, Equatable {
    case unreachable(String)
    case closed
    case cannotUpgrade
}

/// TLS from the first byte, over `Network.framework`.
actor TLSTransport: ByteTransport {
    private let connection: NWConnection

    init(host: String, port: UInt16) {
        connection = NWConnection(
            host: .init(host), port: .init(rawValue: port) ?? 993,
            using: NWParameters(tls: NWProtocolTLS.Options(), tcp: .init()))
    }

    func connect() async throws {
        let once = ResumeOnce()
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Void, Error>) in
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready: once.resume(k, with: .success(()))
                case let .failed(e):
                    once.resume(
                        k, with: .failure(TransportFailure.unreachable(e.localizedDescription)))
                case .cancelled: once.resume(k, with: .failure(TransportFailure.closed))
                default: break
                }
            }
            connection.start(queue: .global(qos: .userInitiated))
        }
    }

    func receive() async throws -> Data {
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Data, Error>) in
            connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) {
                data, _, complete, error in
                if let error {
                    k.resume(throwing: TransportFailure.unreachable(error.localizedDescription))
                    return
                }
                if complete, data?.isEmpty != false {
                    k.resume(throwing: TransportFailure.closed)
                    return
                }
                k.resume(returning: data ?? Data())
            }
        }
    }

    func send(_ data: Data) async throws {
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Void, Error>) in
            connection.send(
                content: data,
                completion: .contentProcessed { error in
                    if let error {
                        k.resume(throwing: TransportFailure.unreachable(error.localizedDescription))
                        return
                    }
                    k.resume()
                })
        }
    }

    /// Already encrypted, and there is nothing to upgrade.
    ///
    /// Not a silent success: a caller that asks has misread the port,
    /// and a `STARTTLS` on a connection that is already TLS is a
    /// command the server is entitled to refuse.
    func upgradeToTLS() async throws { throw TransportFailure.cannotUpgrade }

    func close() { connection.cancel() }
}

/// Plain to begin with, encrypted on request — the submission port.
///
/// `Stream`, not `NWConnection`, for exactly one reason: this is the
/// only API on the platform that will encrypt a socket that is already
/// carrying a conversation, which is what STARTTLS is.
actor UpgradableTransport: ByteTransport {
    private let host: String
    private let port: UInt16
    private var input: InputStream?
    private var output: OutputStream?

    init(host: String, port: UInt16) {
        self.host = host
        self.port = port
    }

    func connect() async throws {
        var readStream: Unmanaged<CFReadStream>?
        var writeStream: Unmanaged<CFWriteStream>?
        CFStreamCreatePairWithSocketToHost(
            nil, host as CFString, UInt32(port), &readStream, &writeStream)
        guard let readStream, let writeStream else {
            throw TransportFailure.unreachable("could not open a connection to \(host)")
        }
        let input = readStream.takeRetainedValue() as InputStream
        let output = writeStream.takeRetainedValue() as OutputStream
        input.open()
        output.open()
        self.input = input
        self.output = output
        try await waitUntilOpen()
    }

    /// Encrypt in place.
    ///
    /// The name is checked against the certificate: without
    /// `kCFStreamSSLPeerName` the socket accepts any certificate a
    /// machine on the path can produce, which is the whole of the
    /// protection gone.
    func upgradeToTLS() async throws {
        guard let input, let output else { throw TransportFailure.closed }
        let settings: [CFString: Any] = [
            kCFStreamSSLLevel: kCFStreamSocketSecurityLevelNegotiatedSSL,
            kCFStreamSSLPeerName: host as CFString,
        ]
        let key = CFStreamPropertyKey(kCFStreamPropertySSLSettings)
        let onInput = CFReadStreamSetProperty(input, key, settings as CFDictionary)
        let onOutput = CFWriteStreamSetProperty(output, key, settings as CFDictionary)
        guard onInput, onOutput else { throw TransportFailure.cannotUpgrade }
        try await waitUntilOpen()
    }

    func receive() async throws -> Data {
        guard let input else { throw TransportFailure.closed }
        // Blocking reads on a background thread rather than the run
        // loop: the session above is `async` and reads one line at a
        // time, and a run-loop-driven stream would need a delegate and
        // a queue to say the same thing.
        return try await withCheckedThrowingContinuation {
            (k: CheckedContinuation<Data, Error>) in
            DispatchQueue.global(qos: .userInitiated).async {
                // Allocated inside, so nothing is shared across the
                // hop — a buffer captured by a concurrent closure is
                // read by one thread while another is filling it.
                var chunk = [UInt8](repeating: 0, count: 16 * 1024)
                let read = input.read(&chunk, maxLength: chunk.count)
                if read < 0 {
                    let why = input.streamError?.localizedDescription ?? "the connection failed"
                    k.resume(throwing: TransportFailure.unreachable(why))
                    return
                }
                if read == 0 {
                    k.resume(throwing: TransportFailure.closed)
                    return
                }
                k.resume(returning: Data(chunk[0..<read]))
            }
        }
    }

    func send(_ data: Data) async throws {
        guard let output else { throw TransportFailure.closed }
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Void, Error>) in
            DispatchQueue.global(qos: .userInitiated).async {
                var sent = 0
                let bytes = [UInt8](data)
                while sent < bytes.count {
                    let wrote = bytes[sent...].withUnsafeBufferPointer { buffer -> Int in
                        guard let base = buffer.baseAddress else { return -1 }
                        return output.write(base, maxLength: buffer.count)
                    }
                    if wrote <= 0 {
                        let why = output.streamError?.localizedDescription ?? "the connection failed"
                        k.resume(throwing: TransportFailure.unreachable(why))
                        return
                    }
                    sent += wrote
                }
                k.resume()
            }
        }
    }

    func close() {
        input?.close()
        output?.close()
        input = nil
        output = nil
    }

    /// Both halves open, or the reason they did not.
    ///
    /// Polled rather than delegated: the same wait serves the first
    /// connection and the TLS handshake, and a delegate would have to
    /// be told which of the two it is watching.
    private func waitUntilOpen(timeout: TimeInterval = 20) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let error = input?.streamError ?? output?.streamError {
                throw TransportFailure.unreachable(error.localizedDescription)
            }
            if input?.streamStatus == .open, output?.streamStatus == .open { return }
            if input?.streamStatus == .error || output?.streamStatus == .error {
                throw TransportFailure.unreachable("the connection failed")
            }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        throw TransportFailure.unreachable("the server did not answer")
    }
}
