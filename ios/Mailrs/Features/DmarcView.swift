import SwiftUI

/// What other mail servers say your mail looked like.
///
/// Deliverability rather than security: mail that did not align is
/// mail a receiver was entitled to reject, so the rate at the top is
/// the number this screen exists for. Backend:
/// `crates/webapi/src/handlers/dmarc.rs`.
struct DmarcView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss

    @State private var reports: [Wire.DmarcReport] = []
    @State private var sources: Wire.DmarcSourceList?
    @State private var loading = true
    @State private var failure: String?

    /// Sources that lose mail first — a forwarder breaking alignment
    /// or somebody sending as you, either of which is the reason to
    /// open this screen.
    private var sortedSources: [Wire.DmarcSource] {
        guard let sources else { return [] }
        return sources.items.sorted { left, right in
            let leftLost = left.total - left.passing
            let rightLost = right.total - right.passing
            if leftLost != rightLost { return leftLost > rightLost }
            return left.total > right.total
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if loading, reports.isEmpty, sources == nil {
                    ProgressView()
                } else if let failure, reports.isEmpty {
                    ContentUnavailableView("Could not load DMARC",
                                           systemImage: "exclamationmark.triangle",
                                           description: Text(failure))
                } else if reports.isEmpty {
                    ContentUnavailableView("No reports yet", systemImage: "chart.bar.doc.horizontal",
                                           description: Text("Receivers send these daily once rua is published."))
                } else {
                    List {
                        if let sources {
                            Section {
                                AlignmentHeadline(passing: sources.passing, total: sources.total)
                            } header: {
                                Text("Alignment")
                            } footer: {
                                Text("\(sources.reports) reports")
                            }
                        }

                        if !sortedSources.isEmpty {
                            Section("Sending sources") {
                                ForEach(sortedSources) { source in
                                    SourceRow(source: source)
                                }
                            }
                        }

                        Section("Reports") {
                            ForEach(reports) { report in
                                ReportRow(report: report)
                            }
                        }
                    }
                    .refreshable { await load() }
                }
            }
            .navigationTitle("DMARC")
            .inlineTitle()
            .toolbar {
                // Trailing, like Settings and Drafts: a sheet with
                // nothing to add has no reason to put its only way out
                // where Cancel lives.
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .task { await load() }
        }
    }

    private func load() async {
        loading = true
        do {
            reports = try await session.dmarcReports()
            sources = try await session.dmarcSources()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
        loading = false
    }
}

/// The window's rate, large, with what it is a rate of underneath.
private struct AlignmentHeadline: View {
    let passing: UInt64
    let total: UInt64

    private var rateText: String {
        guard let text = AlignmentRate.percentText(passing: passing, total: total) else {
            return "—"
        }
        return text
    }

    /// Green only above 99%: DMARC alignment is either essentially
    /// total or something is wrong, and a scale that called 95%
    /// healthy would be calling one message in twenty rejectable fine.
    private var tint: Color {
        guard let fraction = AlignmentRate.fraction(passing: passing, total: total) else {
            return .secondary
        }
        if fraction >= 0.99 { return .green }
        if fraction >= 0.95 { return .orange }
        return .red
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(verbatim: rateText)
                .font(.system(.largeTitle, design: .rounded, weight: .semibold))
                .foregroundStyle(tint)
                .monospacedDigit()
            Text("\(Int(passing)) of \(Int(total)) messages aligned")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
    }
}

private struct SourceRow: View {
    let source: Wire.DmarcSource

    private var lost: UInt64 {
        source.total - source.passing
    }

    private var edgeColor: Color {
        if lost > 0 { return Color.red.opacity(0.6) }
        return .clear
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Rectangle()
                .fill(edgeColor)
                .frame(width: 3)
                .clipShape(Capsule())
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(verbatim: source.sourceIp)
                        .font(.subheadline.weight(.medium))
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(verbatim: AlignmentRate.percentText(
                        passing: source.passing, total: source.total
                    ) ?? "—")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
                Text(verbatim: source.domains.joined(separator: ", "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 2)
    }
}

private struct ReportRow: View {
    let report: Wire.DmarcReport

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(verbatim: report.orgName)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                Spacer(minLength: 4)
                RowDateText(epochSeconds: report.begin, style: .day)
            }
            HStack(spacing: 6) {
                Text(verbatim: report.policyDomain)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                // The published policy, because a 100% rate under
                // `p=none` means nothing was being enforced.
                Text(verbatim: "p=\(report.p)")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color(.tertiarySystemFill), in: Capsule())
                Spacer(minLength: 4)
                Text(verbatim: AlignmentRate.percentText(
                    passing: report.passing, total: report.total
                ) ?? "—")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        .padding(.vertical, 2)
    }
}
