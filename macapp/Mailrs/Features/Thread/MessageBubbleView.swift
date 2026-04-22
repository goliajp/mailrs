import SwiftUI
import QuickLook

struct MessageBubbleView: View {
    let message: ThreadMessage
    let attachmentService: AttachmentService

    @State private var expanded: Bool = true
    @State private var previewURL: URL?
    @State private var downloadingIndex: Int?
    @State private var downloadError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            header
            if expanded {
                metadataRow
                Divider().padding(.vertical, 4)
                body(for: message)
                if !message.attachments.isEmpty {
                    attachmentsSection
                }
            }
        }
        .padding(12)
        #if os(macOS)
        .background(Color(nsColor: .textBackgroundColor).opacity(0.5))
        #else
        .background(Color(uiColor: .secondarySystemBackground))
        #endif
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .quickLookPreview($previewURL)
    }

    private var header: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 2) {
                Text(message.sender)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                Text("至 \(message.recipients)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 2) {
                Text(fullDate(message.internal_date))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                Button {
                    withAnimation { expanded.toggle() }
                } label: {
                    Image(systemName: expanded ? "chevron.up" : "chevron.down")
                        .font(.caption)
                }
                .buttonStyle(.plain)
            }
        }
    }

    @ViewBuilder
    private var metadataRow: some View {
        if message.has_tracking_pixel || message.risk_score > 0.3 || message.is_bulk_sender {
            HStack(spacing: 6) {
                if message.has_tracking_pixel {
                    tag("含追踪像素", color: .orange, systemImage: "eye.trianglebadge.exclamationmark")
                }
                if message.risk_score > 0.3 {
                    tag("风险 \(Int(message.risk_score * 100))%", color: .red, systemImage: "exclamationmark.shield")
                }
                if message.is_bulk_sender {
                    tag("群发", color: .secondary, systemImage: "megaphone")
                }
            }
        }
    }

    private func tag(_ text: String, color: Color, systemImage: String) -> some View {
        HStack(spacing: 3) {
            Image(systemName: systemImage).font(.caption2)
            Text(text).font(.caption2)
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 3)
        .background(color.opacity(0.15))
        .foregroundStyle(color)
        .clipShape(Capsule())
    }

    @ViewBuilder
    private func body(for message: ThreadMessage) -> some View {
        if let html = message.html_body, !html.isEmpty {
            HtmlBodyView(html: html)
        } else {
            let text = message.preferredBodyText
            if text.isEmpty {
                Text("(空消息)")
                    .foregroundStyle(.secondary)
            } else {
                Text(text)
                    .font(.body)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private var attachmentsSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("附件 (\(message.attachments.count))", systemImage: "paperclip")
                .font(.caption.bold())
                .foregroundStyle(.secondary)
            ForEach(Array(message.attachments.enumerated()), id: \.offset) { idx, att in
                attachmentRow(index: idx, attachment: att)
            }
            if let err = downloadError {
                Text(err).font(.caption).foregroundStyle(.red)
            }
        }
        .padding(.top, 8)
    }

    private func attachmentRow(index: Int, attachment: AttachmentInfo) -> some View {
        Button {
            Task { await download(index: index, attachment: attachment) }
        } label: {
            HStack(spacing: 10) {
                Image(systemName: iconName(for: attachment.content_type))
                    .font(.title3)
                    .frame(width: 28)
                    .foregroundStyle(.tint)
                VStack(alignment: .leading, spacing: 1) {
                    Text(attachment.filename).font(.caption).lineLimit(1)
                    Text("\(formatSize(attachment.size)) • \(attachment.content_type)")
                        .font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                if downloadingIndex == index {
                    ProgressView().controlSize(.small)
                } else {
                    Image(systemName: "arrow.down.circle").foregroundStyle(.secondary)
                }
            }
            .padding(8)
            .background(Color.secondary.opacity(0.08))
            .clipShape(RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
    }

    private func download(index: Int, attachment: AttachmentInfo) async {
        downloadingIndex = index
        downloadError = nil
        defer { downloadingIndex = nil }
        do {
            let url = try await attachmentService.download(
                messageUid: message.uid,
                attachmentIndex: index,
                filename: attachment.filename
            )
            previewURL = url
        } catch {
            downloadError = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    private func iconName(for contentType: String) -> String {
        let lc = contentType.lowercased()
        if lc.contains("image") { return "photo" }
        if lc.contains("pdf") { return "doc.richtext" }
        if lc.contains("zip") || lc.contains("compressed") { return "doc.zipper" }
        if lc.contains("audio") { return "waveform" }
        if lc.contains("video") { return "film" }
        if lc.contains("text") || lc.contains("plain") { return "doc.plaintext" }
        return "doc"
    }

    private func formatSize(_ bytes: Int) -> String {
        ByteCountFormatter().string(fromByteCount: Int64(bytes))
    }

    private func fullDate(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .short
        f.locale = Locale(identifier: "zh_CN")
        return f.string(from: date)
    }
}
