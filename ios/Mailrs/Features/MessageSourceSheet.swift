import SwiftUI

/// A message as it arrived — or as it left, headers and all.
///
/// The one screen in this app that shows what the server was given
/// rather than what it made of it — which is where the answer lives
/// when a message landed in Junk, or claims to be from someone it is
/// not. `Authentication-Results` is the line worth finding, and it is
/// near the top.
struct MessageSourceSheet: View {
    /// A received message, fetched by uid.
    var uid: UInt32?
    /// A sent one, already fetched — the Send list asks by send id,
    /// which is not a uid, so it hands the bytes over rather than
    /// teaching this sheet a second way to ask.
    var text: String?
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss
    @State private var source = ""
    @State private var failure: String?
    @State private var loading = true

    var body: some View {
        NavigationStack {
            Group {
                if loading {
                    ProgressView()
                } else if let failure {
                    ContentUnavailableView(
                        "Could not read the source", systemImage: "doc.questionmark",
                        description: Text(failure))
                } else {
                    ScrollView([.horizontal, .vertical]) {
                        // Monospaced and unwrapped: a folded header
                        // means something, and rewrapping it to the
                        // screen width would be showing a different
                        // message than the one that arrived.
                        Text(source)
                            .font(.system(.caption2, design: .monospaced))
                            .textSelection(.enabled)
                            .padding(12)
                    }
                }
            }
            .navigationTitle("Message source")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        UIPasteboard.general.string = source
                    } label: {
                        Label("Copy", systemImage: "doc.on.doc")
                    }
                    .disabled(source.isEmpty)
                }
            }
            .task {
                if let text {
                    source = text
                    loading = false
                    return
                }
                guard let client = session.client, let uid else {
                    failure = "Not signed in."
                    loading = false
                    return
                }
                do {
                    source = try await client.messageSource(uid: uid)
                } catch {
                    failure = error.localizedDescription
                }
                loading = false
            }
        }
    }
}
