import Foundation

/// The word a Sent-list row shows for a send's delivery state.
enum SendStatus {
    /// The catalog key for `status` as `/api/mail/sends` gives it, or
    /// nil where the row says nothing: a send with no record predates
    /// the projection, and a default word would claim knowledge the row
    /// does not have.
    static func labelKey(_ status: String?) -> String? {
        switch status {
        case "failed": "Failed"
        case "partial": "Partial"
        case "sending": "Sending"
        case "scheduled": "Scheduled"
        case "delivered": "Delivered"
        case "cancelled": "Cancelled"
        default: nil
        }
    }
}
