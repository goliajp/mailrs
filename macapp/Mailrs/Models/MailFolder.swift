import Foundation

enum MailFolder: String, CaseIterable, Identifiable, Hashable, Sendable {
    case inbox
    case sent
    case drafts
    case trash

    var id: String { rawValue }

    /// Backend folder name. `nil` means "no `folder` query param" (which the
    /// server treats as inbox).
    var queryValue: String? {
        switch self {
        case .inbox: return nil
        case .sent: return "Sent"
        case .drafts: return "Drafts"
        case .trash: return "Trash"
        }
    }

    var displayName: String {
        switch self {
        case .inbox: return "收件箱"
        case .sent: return "已发送"
        case .drafts: return "草稿"
        case .trash: return "废纸篓"
        }
    }

    var systemImage: String {
        switch self {
        case .inbox: return "tray"
        case .sent: return "paperplane"
        case .drafts: return "doc"
        case .trash: return "trash"
        }
    }
}

enum MailCategory: String, CaseIterable, Identifiable, Hashable, Sendable {
    case personal
    case promotion
    case notification
    case general
    case spam
    case scam

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .personal: return "个人"
        case .promotion: return "促销"
        case .notification: return "通知"
        case .general: return "普通"
        case .spam: return "垃圾"
        case .scam: return "诈骗"
        }
    }

    var systemImage: String {
        switch self {
        case .personal: return "person"
        case .promotion: return "tag"
        case .notification: return "bell"
        case .general: return "envelope"
        case .spam: return "xmark.bin"
        case .scam: return "exclamationmark.shield"
        }
    }
}

enum QuickFilter: String, CaseIterable, Identifiable, Hashable, Sendable {
    case all
    case unread
    case starred
    case attachment

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .all: return "全部"
        case .unread: return "未读"
        case .starred: return "星标"
        case .attachment: return "附件"
        }
    }

    var systemImage: String {
        switch self {
        case .all: return "circle"
        case .unread: return "circle.inset.filled"
        case .starred: return "star"
        case .attachment: return "paperclip"
        }
    }
}

enum SortOrder: String, CaseIterable, Identifiable, Hashable, Sendable {
    case newest
    case oldest
    case unread

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .newest: return "最新优先"
        case .oldest: return "最早优先"
        case .unread: return "未读优先"
        }
    }
}
