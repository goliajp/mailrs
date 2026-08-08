import Foundation

/// Whether a message reaches off the device to render.
///
/// Every remote reference in mail is a beacon whether or not it was
/// meant as one: fetching it tells the sender the message was opened,
/// from which address, at what time, on what network. The backend
/// already flags the obvious `has_tracking_pixel` case; this is the
/// general one, because a 1×1 gif and a logo report identically.
///
/// Deliberately generous about what counts — a missed reference is a
/// silent leak, while a false positive is one banner on a message
/// that had nothing to load.
enum RemoteContent {
    static func hasRemoteReferences(html: String) -> Bool {
        let lowered = html.lowercased()
        for marker in ["src=\"http", "src='http", "src=http",
                       "src=\"//", "src='//",
                       "background=\"http", "background='http",
                       "url(http", "url('http", "url(\"http",
                       "url(//", "url('//", "url(\"//"] where lowered.contains(marker) {
            return true
        }
        return false
    }
}
