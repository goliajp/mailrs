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
                // Trailing, like Settings and Drafts: a sheet with
                // nothing to add has no reason to put its only way out
                // where Cancel lives.
                ToolbarItem(placement: .confirmationAction) {
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

    private var timing: QueueTiming {
        QueueTiming.of(status: job.status, scheduledAt: job.scheduledAt,
                       nextRetry: job.nextRetry, createdAt: job.createdAt,
                       now: Int64(Date().timeIntervalSince1970))
    }

    /// "Waiting" was the whole status line, and it repeated the word
    /// already at the top of the screen. A scheduled send says so.
    private var statusLabel: LocalizedStringKey {
        switch timing {
        case .inflight: return "Sending"
        case .scheduled: return "Scheduled"
        case .retrying: return "Retrying"
        case .queued, .unknown: return "Waiting"
        }
    }

    private var statusTint: Color {
        if timing.isScheduled { return .accentColor }
        return .secondary
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
                        .foregroundStyle(statusTint)
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
                QueueTimingLine(timing: timing, attempts: job.attempts)
            }
        }
        .padding(.vertical, 2)
    }
}

/// One line for the timing, because the row already has four.
///
/// The most decision-relevant fact leads and the attempt count trails,
/// so a narrow phone truncates the tail rather than wrapping — a row
/// forced onto a second line is the defect this app does not ship.
private struct QueueTimingLine: View {
    let timing: QueueTiming
    let attempts: Int?

    private var symbol: String {
        switch timing {
        case .scheduled: return "clock.badge.checkmark"
        case .retrying: return "arrow.clockwise"
        case .queued: return "clock"
        case .inflight: return "paperplane"
        case .unknown: return "clock"
        }
    }

    var body: some View {
        if let epoch = timing.epochSeconds {
            HStack(spacing: 4) {
                Image(systemName: symbol)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                phrase(epoch)
                if let attempts, attempts > 0 {
                    Text(verbatim: "·").foregroundStyle(.tertiary)
                    Text("\(attempts) attempts").foregroundStyle(.secondary)
                }
            }
            .font(.caption2)
            .lineLimit(1)
        }
    }

    /// The date is a string here rather than a `RowDateText`, because a
    /// sentence with a date in it has to stay one run of text — two
    /// views side by side would break between the words.
    @ViewBuilder private func phrase(_ epoch: Int64) -> some View {
        let when = RowDate.label(epochSeconds: epoch, calendar: reader)
        switch timing {
        case .scheduled: Text("Sends \(when)").foregroundStyle(Color.accentColor)
        case .retrying: Text("Next attempt \(when)").foregroundStyle(.secondary)
        default: Text("Queued \(when)").foregroundStyle(.secondary)
        }
    }

    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone
    @Environment(\.locale) private var locale

    private var reader: Calendar { .reader(calendar, timeZone, locale) }
}
