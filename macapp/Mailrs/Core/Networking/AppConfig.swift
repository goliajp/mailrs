import Foundation

enum AppConfig {
    static let apiBaseURL: URL = URL(string: "https://mail.golia.ai")!

    static let webSocketBaseURL: URL = URL(string: "wss://mail.golia.ai")!

    static let appGroupService = "jp.golia.mailrs"
}
