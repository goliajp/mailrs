import SwiftUI

/// What has not left yet, and who the sender has given up on.
///
/// The operational question a phone is actually for: is anything
/// stuck. Backend: `crates/webapi/src/handlers/complete.rs`
/// (`list_admin_queue`) and `admin_ops.rs` (suppressions).
struct QueueView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var jobs: [Wire.QueueJob] = []
    @State private var suppressed: [String] = []
    @State private var loading = true
    @State private var failure: String?
    @State private var confirmingClear = false

    /// Failures first: a job with an error is the reason anyone opened
    /// this screen, and it must not be below fifty healthy ones.
    private var sortedJobs: [Wire.QueueJob] {
        jobs.sorted { left, right in
            let leftFailed = left.lastError?.isEmpty == false
            let rightFailed = right.lastError?.isEmpty == false
            if leftFailed != rightFailed { return leftFailed }
            return left.id > right.id
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if loading, jobs.isEmpty, suppressed.isEmpty {
                    ProgressView()
                } else if let failure, jobs.isEmpty {
                    ContentUnavailableView("Could not load the queue",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(failure))
                } else {
                    List {
                        Section {
                            if jobs.isEmpty {
                                // A said "nothing waiting" rather than an
                                // empty section: the absence is the good
                                // news, and it should read as an answer.
                                Label("Nothing waiting", systemImage: "checkmark.circle")
                                    .foregroundStyle(.secondary)
                            } else {
                                ForEach(sortedJobs) { job in
                                    QueueRow(job: job)
                                }
                            }
                        } header: {
                            Text("Outbound")
                        }

                        Section {
                            if suppressed.isEmpty {
                                Label("No suppressed addresses", systemImage: "checkmark.circle")
                                    .foregroundStyle(.secondary)
                            } else {
                                ForEach(suppressed, id: \.self) { address in
                                    Text(verbatim: address).font(.subheadline)
                                }
                                Button(role: .destructive) {
                                    confirmingClear = true
                                } label: {
                                    Text("Clear all")
                                }
                            }
                        } header: {
                            Text("Suppressed")
                        } footer: {
                            Text("Addresses the sender will not try again until cleared.")
                        }
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("Queue")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .alert("Clear all suppressions?", isPresented: $confirmingClear) {
                Button("Clear", role: .destructive) { Task { await clear() } }
                Button("Cancel", role: .cancel) {}
            } message: {
                // The endpoint takes no address — it is the whole set or
                // nothing, and that is worth saying before it happens.
                Text("Every suppressed address becomes deliverable again.")
            }
            .task { await load() }
        }
    }

    private func load() async {
        loading = true
        do {
            jobs = try await session.queue()
            suppressed = try await session.suppressions()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }

    private func clear() async {
        do {
            try await session.clearSuppressions()
            await load()
        } catch {
            failure = error.localizedDescription
        }
    }
}

private struct QueueRow: View {
    let job: Wire.QueueJob

    private var failed: Bool {
        job.lastError?.isEmpty == false
    }

    private var edgeColor: Color {
        if failed { return Color.red.opacity(0.6) }
        return .clear
    }

    private var statusLabel: LocalizedStringKey {
        if job.status == "inflight" { return "Sending" }
        return "Waiting"
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Rectangle()
                .fill(edgeColor)
                .frame(width: 3)
                .clipShape(Capsule())
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(verbatim: job.recipient)
                        .font(.subheadline.weight(.medium))
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(statusLabel)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                Text(verbatim: job.sender)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                if let error = job.lastError, !error.isEmpty {
                    // The server's own words: a queue screen that
                    // paraphrased the failure would be the one place
                    // the real reason is not written down.
                    Text(verbatim: error)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .lineLimit(3)
                }
                if let attempts = job.attempts, attempts > 0 {
                    Text("\(attempts) attempts")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 2)
    }
}
