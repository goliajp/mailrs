import Foundation

/// One address a message can leave by.
struct FromAddress: Equatable, Identifiable {
    let accountId: String
    let address: String
    let label: String

    var id: String { accountId }
}

/// Every address this person can send as, this server's own first.
///
/// An account whose credential was refused is left out: choosing it
/// would produce a message that cannot be sent, and offering a choice
/// that fails is worse than not offering it.
func fromAddresses(own: String, accounts: [Wire.ExternalAccount]) -> [FromAddress] {
    var out: [FromAddress] = []
    if !own.isEmpty { out.append(FromAddress(accountId: "", address: own, label: own)) }
    for a in accounts where a.state != "needs_auth" {
        let name = a.displayName
        let label = (!name.isEmpty && name != a.email) ? "\(name) · \(a.email)" : a.email
        out.append(FromAddress(accountId: a.id, address: a.email, label: label))
    }
    return out
}

/// The address a reply should leave by, given where the mail arrived.
///
/// Not "the account you signed in as". A reply to mail that arrived at
/// a connected Gmail has to go out through that Gmail — sent from
/// anywhere else it lands in the conversation as a stranger, and half
/// the time the recipient's provider refuses it outright.
///
/// Falls back to this server's address when the conversation came from
/// an account that is gone or cannot send: replying from somewhere
/// beats a composer that will not send.
func replyFromFor(_ accountId: String?, addresses: [FromAddress]) -> String {
    addresses.first { $0.accountId == (accountId ?? "") }?.address
        ?? addresses.first?.address
        ?? ""
}

/// The second line of an account row, or nil.
///
/// An account with no name of its own falls back to its address on the
/// first line — so repeating the address underneath says nothing and
/// reads as a rendering fault. The test that caught it on the other
/// phone found two nodes carrying the same text.
func accountSubtitle(displayName: String, email: String) -> String? {
    (displayName.isEmpty || displayName == email) ? nil : email
}
