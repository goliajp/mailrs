import Foundation

struct AttachmentService {
    let baseURL: URL
    let tokenProvider: TokenProvider
    let session: URLSession

    init(baseURL: URL, tokenProvider: TokenProvider, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.tokenProvider = tokenProvider
        self.session = session
    }

    /// Builds an authenticated URL for inline use (e.g. `<img>` in HTML bodies).
    /// Puts the token in the query string because HTML images can't set headers.
    func inlineURL(messageUid: Int, attachmentIndex: Int, token: String) -> URL? {
        var comps = URLComponents(url: baseURL, resolvingAgainstBaseURL: false)
        comps?.path = "/api/mail/messages/\(messageUid)/attachments/\(attachmentIndex)"
        comps?.queryItems = [URLQueryItem(name: "token", value: token)]
        return comps?.url
    }

    /// Downloads an attachment to a cache directory and returns the file URL.
    func download(messageUid: Int, attachmentIndex: Int, filename: String,
                  progress: ((Double) -> Void)? = nil) async throws -> URL {
        var comps = URLComponents(url: baseURL, resolvingAgainstBaseURL: false)!
        comps.path = "/api/mail/messages/\(messageUid)/attachments/\(attachmentIndex)"
        guard let url = comps.url else { throw ApiError.invalidURL }

        var req = URLRequest(url: url)
        if let token = await tokenProvider.currentToken() {
            req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }

        // URLSession.download(for:) doesn't report progress via the closure; use delegate-less approach for simplicity.
        let (tempURL, response) = try await session.download(for: req)
        guard let http = response as? HTTPURLResponse, 200..<300 ~= http.statusCode else {
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            throw ApiError.server(status: status, message: "Download failed")
        }

        let cacheDir = try cacheDirectory()
        let safeName = filename.replacingOccurrences(of: "/", with: "_")
        let destination = cacheDir.appendingPathComponent("\(messageUid)-\(attachmentIndex)-\(safeName)")
        try? FileManager.default.removeItem(at: destination)
        try FileManager.default.moveItem(at: tempURL, to: destination)
        progress?(1.0)
        return destination
    }

    private func cacheDirectory() throws -> URL {
        let fm = FileManager.default
        let base = try fm.url(for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let dir = base.appendingPathComponent("attachments", isDirectory: true)
        if !fm.fileExists(atPath: dir.path) {
            try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        }
        return dir
    }
}
