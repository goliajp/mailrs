import SwiftUI

/// Why this message was held, in the four layers that decided it.
///
/// Nothing here recomputes. It draws the verdict stored when the mail
/// arrived, with the rule-set version beside it. Every reason is a code
/// the server stored; the words come from the shared reasons table in
/// the reader's language, and a code this build has no sentence for
/// falls back to the server's English detail. Same panel as the web's.
struct VerdictPanel: View {
    let messageId: String
    let threadId: String
    @Environment(Session.self) private var session
    @Environment(\.locale) private var locale
    /// Fetched by the card, which is always on screen: a `.task` on this
    /// view would never run, because without a verdict it draws nothing.
    @Binding var verdict: Wire.FraudVerdict?
    @State private var releasing = false

    private var lang: ReasonText.Lang { ReasonText.Lang.of(locale) }

    var body: some View { content }

    // Almost every message has no verdict, and draws nothing: an
    // "examined, nothing found" banner on every mail would make the ones
    // that matter invisible.
    @ViewBuilder
    private var content: some View {
        if let verdict {
            if verdict.layers.isEmpty {
                // mail nobody suspected, sorted by a rule, the classifier or a filter
                if !verdict.placement.isEmpty { placementNote(verdict.placement) }
            } else {
                panel(verdict)
            }
        }
    }

    private func panel(_ verdict: Wire.FraudVerdict) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top, spacing: 8) {
                Image(systemName: "exclamationmark.shield.fill")
                    .foregroundStyle(.red)
                VStack(alignment: .leading, spacing: 2) {
                    title(verdict.quarantined)
                        .font(.footnote.weight(.semibold))
                    Text("Kept, not deleted — this is the evidence an abuse report is built from.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(10)
            if !verdict.placement.isEmpty {
                Divider()
                VStack(alignment: .leading, spacing: 2) {
                    Text("Why it is here").font(.caption.weight(.medium))
                    reasonList(verdict.placement)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
            }
            Divider()
            VStack(alignment: .leading, spacing: 6) {
                ForEach(verdict.layers, id: \.name) { layer in layerRow(layer) }
            }
            .padding(10)
            Divider()
            HStack {
                Text("rules \(verdict.rulesVersion)")
                    .font(.caption2.monospaced())
                    .foregroundStyle(.secondary)
                Spacer()
                if verdict.quarantined { releaseButton }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
        }
        .background(Color.red.opacity(0.06), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.secondary.opacity(0.25)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("verdict-panel")
    }

    @ViewBuilder
    private func title(_ quarantined: Bool) -> some View {
        if quarantined {
            Text("Held: suspected fraud")
        } else {
            Text("Examined: something looked wrong")
        }
    }

    private var releaseButton: some View {
        Button {
            releasing = true
            Task {
                if await session.releaseHeld(threadId: threadId) {
                    verdict = await session.fraudVerdict(messageId: messageId)
                }
                releasing = false
            }
        } label: {
            releaseLabel.font(.caption.weight(.medium))
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .disabled(releasing)
        .accessibilityIdentifier("verdict-release")
    }

    @ViewBuilder
    private var releaseLabel: some View {
        if releasing {
            Text("Releasing…")
        } else {
            Text("This is not fraud")
        }
    }

    private func layerRow(_ layer: Wire.FraudLayer) -> some View {
        HStack(alignment: .top, spacing: 8) {
            outcomeIcon(layer.outcome)
                .font(.caption)
            VStack(alignment: .leading, spacing: 1) {
                HStack(alignment: .firstTextBaseline) {
                    layerTitle(layer.name).font(.caption.weight(.medium))
                    Spacer()
                    if layer.score > 0 {
                        scoreText(layer.score)
                    }
                }
                layerQuestion(layer.name)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                layerReasons(layer)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("verdict-layer-\(layer.name)")
    }

    /// What the layer saw. The server's English detail stands in when the
    /// verdict predates reasons or carries a code this build has no
    /// sentence for.
    @ViewBuilder
    private func layerReasons(_ layer: Wire.FraudLayer) -> some View {
        let table = ReasonText.bundled
        if layer.reasons.isEmpty && layer.outcome == "pass" {
            Text("Nothing found.").font(.caption2)
        } else if layer.reasons.isEmpty || !layer.reasons.allSatisfy({ table.has($0.code) }) {
            // a layer that was never asked has nothing to say, not an empty line
            if !layer.detail.isEmpty { Text(verbatim: layer.detail).font(.caption2) }
        } else {
            reasonList(layer.reasons)
        }
    }

    /// Each line beside what it added to its layer, so the layer's score
    /// is the sum of the numbers the reader sees under it.
    private func reasonList(_ reasons: [Wire.Reason]) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            ForEach(Array(reasons.enumerated()), id: \.offset) { _, reason in
                HStack(alignment: .firstTextBaseline) {
                    Text(verbatim: ReasonText.bundled.words(reason, lang))
                        .font(.caption2)
                        .fixedSize(horizontal: false, vertical: true)
                    if let score = reason.score, score > 0 {
                        Spacer()
                        scoreText(score)
                    }
                }
            }
        }
    }

    private func scoreText(_ score: Double) -> some View {
        Text(verbatim: Self.scoreLabel(score))
            .font(.caption2.monospaced())
            .foregroundStyle(.secondary)
    }

    /// `+4.5`: how a layer's score and each of its lines are written.
    static func scoreLabel(_ score: Double) -> String {
        "+" + String(format: "%.1f", score)
    }

    private func placementNote(_ reasons: [Wire.Reason]) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "info.circle").foregroundStyle(.secondary).font(.caption)
            VStack(alignment: .leading, spacing: 2) {
                Text("Why it is here").font(.caption.weight(.medium))
                reasonList(reasons)
            }
            Spacer(minLength: 0)
        }
        .padding(10)
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.secondary.opacity(0.25)))
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("placement-note")
    }

    // Not a tick for "not applicable": "nothing was checked" and
    // "everything was fine" are different facts.
    @ViewBuilder
    private func outcomeIcon(_ outcome: String) -> some View {
        switch outcome {
        case "fail":
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
        case "pass":
            Image(systemName: "checkmark.circle").foregroundStyle(.green)
        default:
            Image(systemName: "minus.circle").foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func layerTitle(_ name: String) -> some View {
        switch name {
        case "transport": Text("Transport")
        case "identity": Text("Identity")
        case "provenance": Text("Provenance")
        case "content": Text("Content")
        default: Text(verbatim: name)
        }
    }

    @ViewBuilder
    private func layerQuestion(_ name: String) -> some View {
        switch name {
        case "transport": Text("Did it come from where it says?")
        case "identity": Text("Is the name it shows a claim it can make?")
        case "provenance": Text("Did a real mail client write it?")
        case "content": Text("Does it read like the fraud it is?")
        default: EmptyView()
        }
    }
}

/// Why the sender is suspicious: what SPF, DKIM and DMARC each said, and
/// whether the name renders reordered. Shown under a suspicious sender
/// only, beside the warning mark, so the reader sees which check failed.
struct SenderChecksLine: View {
    let trust: String
    let checks: [Wire.Reason]
    @Environment(\.locale) private var locale

    var body: some View {
        if trust == "suspicious" && !checks.isEmpty {
            let lang = ReasonText.Lang.of(locale)
            HStack(spacing: 8) {
                ForEach(Array(checks.enumerated()), id: \.offset) { _, check in
                    Text(verbatim: ReasonText.bundled.words(check, lang))
                        .font(.caption2)
                        .foregroundStyle(color(check))
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("sender-checks")
        }
    }

    private func color(_ check: Wire.Reason) -> Color {
        if check.code == "auth-check" && check.params["result"] == "pass" { return .secondary }
        return .red
    }
}

/// The phone numbers a message gives that its sender has not given the
/// reader before, in this conversation or any other. A notice, not a verdict: a number proves
/// nothing about who answers it, and invoices carry them too.
struct CallbackNotice: View {
    let numbers: [String]

    var body: some View {
        if !numbers.isEmpty {
            Label {
                Text("This message gives a phone number its sender has not given you before: \(numbers.joined(separator: ", ")). Before calling about a payment or an account, look the number up yourself.")
            } icon: {
                Image(systemName: "phone")
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("callback-notice")
        }
    }
}
