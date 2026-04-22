import Foundation

enum ThemeMode: String, CaseIterable, Identifiable, Sendable {
    case system
    case light
    case dark

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .system: return "跟随系统"
        case .light: return "浅色"
        case .dark: return "深色"
        }
    }
}

enum SettingsKey {
    static let theme = "mailrs_theme"
    static let notificationsEnabled = "mailrs_notifications_enabled"
    static let notificationSound = "mailrs_notification_sound"
    static let signature = "mailrs_signature"
    static let signatureEnabled = "mailrs_signature_enabled"
}
