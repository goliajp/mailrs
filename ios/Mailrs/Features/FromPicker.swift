import SwiftUI

/// Which address a message leaves by.
///
/// Only shown when there is more than one to choose between — with a
/// single mailbox the control is furniture, and the address it would
/// show is the one already implied.
struct FromPicker: View {
    let addresses: [FromAddress]
    @Binding var selection: String

    var body: some View {
        if addresses.count > 1 {
            Picker("From", selection: $selection) {
                ForEach(addresses) { a in
                    Text(a.label).tag(a.address)
                }
            }
            .accessibilityIdentifier("compose.from")
        }
    }
}

/// The addresses this person can send as, loaded once per composer.
///
/// A composer that cannot reach the list still sends: it falls back to
/// the signed-in address, which is what every message did before there
/// was anything to choose.
@MainActor
func loadFromAddresses(session: Session, own: String) async -> [FromAddress] {
    guard let client = session.client else {
        return fromAddresses(own: own, accounts: [])
    }
    let accounts = (try? await client.externalAccounts()) ?? []
    return fromAddresses(own: own, accounts: accounts)
}
