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

    var body: some View {
        NavigationStack {
            Group {
                if session.drafts.isEmpty {
                    ContentUnavailableView("No drafts", systemImage: "doc.text")
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
                                        Text(draft.to.isEmpty ? "No recipient" : draft.to)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                    }
                                }
                            }
                            .buttonStyle(.plain)
                        }
                        .onDelete { offsets in
                            let ids = offsets.map { session.drafts[$0].id }
                            Task { for id in ids { await session.deleteDraft(id: id) } }
                        }
                    }
                }
            }
            .navigationTitle("Drafts")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .task { await session.loadDrafts() }
            .sheet(item: $resuming) { draft in
                ComposeView(resuming: draft)
            }
        }
    }
}
