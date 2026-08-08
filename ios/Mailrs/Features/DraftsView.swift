import SwiftUI

/// The unsent messages, newest first.
///
/// Tapping one reopens the composer on it — same view, same autosave, so
/// resuming a draft and starting one differ only in what the fields
/// begin with.
struct DraftsView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss
    @State private var resuming: Wire.Draft?
    /// This sheet was written after the mail lists and did not inherit
    /// their gate: "No drafts" appeared the instant it opened, while the
    /// request was still out, and then the drafts arrived underneath it.
    /// An empty state is a conclusion and it waits for the evidence.
    @State private var loading = true
    @State private var pendingDelete: Wire.Draft?

    var body: some View {
        NavigationStack {
            Group {
                if loading, session.drafts.isEmpty {
                    ProgressView()
                } else if let failure = session.draftsFailure, session.drafts.isEmpty {
                    ContentUnavailableView("Could not load drafts",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(verbatim: failure))
                } else if session.drafts.isEmpty {
                    ContentUnavailableView(
                        "No drafts", systemImage: "doc.text",
                        description: Text("A message you close without sending is kept here.")
                    )
                } else {
                    List {
                        ForEach(session.drafts) { draft in
                            Button {
                                resuming = draft
                            } label: {
                                HStack(spacing: 10) {
                                    // A draft has no correspondent yet;
                                    // its face is the document.
                                    Image(systemName: "doc.text.fill")
                                        .font(.system(size: 17))
                                        .foregroundStyle(Color.accentColor)
                                        .frame(width: 36, height: 36)
                                        .background(Color.accentColor.opacity(0.12), in: Circle())
                                        .accessibilityHidden(true)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(DraftRule.title(subject: draft.subject, body: draft.body))
                                            .font(.subheadline)
                                            .lineLimit(1)
                                        ValueOrPlaceholder(value: draft.to,
                                                           placeholder: "No recipient")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                    }
                                }
                            }
                            .buttonStyle(.plain)
                        }
                        .onDelete { offsets in
                            // One at a time, named, because a draft is
                            // the least recoverable thing in the app —
                            // deleting an alias asks, and an alias takes
                            // five seconds to retype.
                            pendingDelete = offsets.first.map { session.drafts[$0] }
                        }
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("Drafts")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .task { await load() }
            .sheet(item: $resuming) { draft in
                ComposeView(resuming: draft)
            }
            .alert("Delete draft?", isPresented: deleteBinding, presenting: pendingDelete) { draft in
                Button("Delete", role: .destructive) {
                    let id = draft.id
                    Task { await session.deleteDraft(id: id) }
                    pendingDelete = nil
                }
                Button("Cancel", role: .cancel) { pendingDelete = nil }
            } message: { draft in
                Text(verbatim: DraftRule.title(subject: draft.subject, body: draft.body))
            }
        }
    }

    private var deleteBinding: Binding<Bool> {
        Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } })
    }

    private func load() async {
        loading = true
        await session.loadDrafts()
        loading = false
    }
}
