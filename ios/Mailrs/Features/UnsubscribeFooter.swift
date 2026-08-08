import SwiftUI

/// The way off a mailing list, under the message that came from one.
///
/// At the foot of the card rather than a banner over it: 42.6% of real
/// mail carries `List-Unsubscribe`, so a banner would be a stripe over
/// nearly every other message, and the reader who wants out has already
/// finished reading. The sender's own unsubscribe link is usually in
/// the same place and unfindable at phone size — this is the same
/// action, in a fixed spot, at a legible size.
struct UnsubscribeFooter: View {
    let threadId: String
    let message: Wire.Message
    @Environment(Session.self) private var session
    @Environment(\.openURL) private var openURL
    @State private var state: Progress = .idle

    private enum Progress: Equatable {
        case idle
        case working
        /// The sender's endpoint refused, or could not be reached. Said
        /// out loud, with the link still there: a failed unsubscribe
        /// that looks like a successful one is how people end up
        /// tapping it every week for a year.
        case failed
        case done
    }

    private var offer: UnsubscribeOffer { UnsubscribeOffer.of(message.unsubscribe) }

    var body: some View {
        if offer.isAvailable {
            VStack(alignment: .leading, spacing: 4) {
                content
            }
            .padding(.top, 2)
        }
    }

    @ViewBuilder
    private var content: some View {
        switch state {
        case .done:
            Label("Unsubscribed", systemImage: "checkmark.circle")
                .font(.caption)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("unsubscribed")
        case .working:
            HStack(spacing: 6) {
                ProgressView().controlSize(.mini)
                Text("Unsubscribing…").font(.caption).foregroundStyle(.secondary)
            }
        case .idle, .failed:
            button
            if state == .failed {
                Text("The sender's list did not accept it. Their link is above.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("unsubscribe-failed")
            }
        }
    }

    private var button: some View {
        Button {
            act()
        } label: {
            Text(offer.label)
                .font(.caption)
                .foregroundStyle(.secondary)
                .underline()
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("unsubscribe")
    }

    private func act() {
        switch offer {
        case .oneClick:
            state = .working
            Task {
                let ok = await session.unsubscribe(threadId: threadId, uid: message.uid)
                if ok {
                    state = .done
                    return
                }
                state = .failed
            }
        // Opened, never fetched. Loading the page is what tells the
        // sender the address is live, and that is the reader's call to
        // make in the open rather than this app's to make quietly.
        case .openPage(let url), .sendMail(let url):
            openURL(url)
        case .none:
            break
        }
    }
}
