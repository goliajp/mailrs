import SwiftUI
import UniformTypeIdentifiers

struct ComposeView: View {
    @Bindable var model: ComposeModel
    @Environment(\.dismiss) private var dismiss
    @Environment(AppModel.self) private var app

    let onSent: () -> Void

    @State private var showingFilePicker = false
    @State private var showingScheduleSheet = false

    var body: some View {
        NavigationStack {
            Form {
                recipientSection
                subjectSection
                bodySection
                attachmentsSection
                quotedSection
                statusSection
            }
            #if os(iOS)
            .formStyle(.grouped)
            #endif
            .navigationTitle(titleForMode)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar { toolbarContent }
            .fileImporter(
                isPresented: $showingFilePicker,
                allowedContentTypes: [.item],
                allowsMultipleSelection: true
            ) { result in
                handleFilePick(result)
            }
            .sheet(isPresented: $showingScheduleSheet) {
                ScheduleSheet(initial: model.scheduledAt ?? .now.addingTimeInterval(3600)) { date in
                    model.scheduledAt = date
                }
            }
            .task {
                model.signature = UserDefaults.standard.string(forKey: SettingsKey.signature) ?? ""
                model.signatureEnabled = UserDefaults.standard.bool(forKey: SettingsKey.signatureEnabled)
            }
        }
        #if os(macOS)
        .frame(minWidth: 640, idealWidth: 720, minHeight: 560, idealHeight: 620)
        #endif
    }

    private var titleForMode: String {
        switch model.mode {
        case .new: return "写邮件"
        case .reply: return "回复"
        case .replyAll: return "回复全部"
        case .forward: return "转发"
        }
    }

    // MARK: form sections

    private var recipientSection: some View {
        Section {
            recipientRow(label: "收件人", text: $model.toText)
            if model.showCc {
                recipientRow(label: "抄送", text: $model.ccText)
            }
            if model.showBcc {
                recipientRow(label: "密送", text: $model.bccText)
            }
            if !(model.showCc && model.showBcc) {
                HStack {
                    if !model.showCc {
                        Button("添加抄送") { model.showCc = true }
                            .buttonStyle(.plain)
                            .foregroundStyle(.tint)
                            .font(.caption)
                    }
                    if !model.showBcc {
                        Button("添加密送") { model.showBcc = true }
                            .buttonStyle(.plain)
                            .foregroundStyle(.tint)
                            .font(.caption)
                    }
                }
            }
        }
    }

    private func recipientRow(label: String, text: Binding<String>) -> some View {
        HStack(alignment: .top) {
            Text(label).frame(width: 60, alignment: .leading).foregroundStyle(.secondary)
            TextField("多个地址用逗号分隔", text: text, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...3)
                #if os(iOS)
                .textContentType(.emailAddress)
                .keyboardType(.emailAddress)
                .autocapitalization(.none)
                .autocorrectionDisabled(true)
                #endif
        }
    }

    private var subjectSection: some View {
        Section {
            HStack {
                Text("主题").frame(width: 60, alignment: .leading).foregroundStyle(.secondary)
                TextField("主题", text: $model.subject)
                    .textFieldStyle(.plain)
            }
        }
    }

    private var bodySection: some View {
        Section("正文") {
            TextEditor(text: $model.body)
                .frame(minHeight: 180)
                .font(.body)
        }
    }

    @ViewBuilder
    private var attachmentsSection: some View {
        if !model.attachments.isEmpty {
            Section("附件 (\(model.attachments.count))") {
                ForEach(model.attachments) { att in
                    HStack(spacing: 10) {
                        Image(systemName: "paperclip")
                        VStack(alignment: .leading, spacing: 1) {
                            Text(att.filename).font(.caption)
                            Text("\(formatSize(att.size)) • \(att.contentType)")
                                .font(.caption2).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button(role: .destructive) {
                            model.removeAttachment(att)
                        } label: {
                            Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var quotedSection: some View {
        if let q = model.quoted {
            Section("引用") {
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(q.sender) • \(q.date.formatted(date: .abbreviated, time: .shortened))")
                        .font(.caption).foregroundStyle(.secondary)
                    Text(q.text)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(4)
                }
            }
        }
    }

    @ViewBuilder
    private var statusSection: some View {
        if let err = model.errorMessage {
            Section {
                Label(err, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                    .font(.caption)
            }
        }
        if let ok = model.successMessage {
            Section {
                Label(ok, systemImage: "checkmark.circle")
                    .foregroundStyle(.green)
                    .font(.caption)
            }
        }
        if let scheduled = model.scheduledAt {
            Section {
                HStack {
                    Image(systemName: "clock")
                    Text("定时发送: \(scheduled.formatted(date: .abbreviated, time: .shortened))")
                        .font(.caption)
                    Spacer()
                    Button("取消") { model.scheduledAt = nil }
                        .buttonStyle(.plain)
                        .foregroundStyle(.tint)
                        .font(.caption)
                }
            }
        }
    }

    // MARK: toolbar

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) {
            Button("取消") { dismiss() }
        }
        ToolbarItem(placement: .primaryAction) {
            Button {
                showingFilePicker = true
            } label: {
                Label("附件", systemImage: "paperclip")
            }
            .disabled(model.isSending)
        }
        ToolbarItem(placement: .primaryAction) {
            Menu {
                Button {
                    showingScheduleSheet = true
                } label: {
                    Label(model.scheduledAt == nil ? "定时发送…" : "修改定时…",
                          systemImage: "clock")
                }
                if model.scheduledAt != nil {
                    Button("取消定时", role: .destructive) {
                        model.scheduledAt = nil
                    }
                }
                Divider()
                Button {
                    Task { _ = await model.saveDraft() }
                } label: {
                    Label("保存草稿", systemImage: "doc.badge.arrow.up")
                }
            } label: {
                Label("更多", systemImage: "ellipsis.circle")
            }
        }
        ToolbarItem(placement: .primaryAction) {
            Button {
                Task {
                    if await model.send() {
                        onSent()
                        dismiss()
                    }
                }
            } label: {
                if model.isSending {
                    ProgressView().controlSize(.small)
                } else {
                    Label(model.scheduledAt == nil ? "发送" : "定时发送",
                          systemImage: "paperplane.fill")
                }
            }
            .disabled(model.isSending || model.toText.isEmpty)
        }
    }

    // MARK: actions

    private func handleFilePick(_ result: Result<[URL], Error>) {
        switch result {
        case .success(let urls):
            for url in urls {
                let accessed = url.startAccessingSecurityScopedResource()
                defer { if accessed { url.stopAccessingSecurityScopedResource() } }
                do {
                    try model.addAttachment(url: url)
                } catch {
                    model.errorMessage = error.localizedDescription
                }
            }
        case .failure(let error):
            model.errorMessage = error.localizedDescription
        }
    }

    private func formatSize(_ bytes: Int) -> String {
        ByteCountFormatter().string(fromByteCount: Int64(bytes))
    }
}

struct ScheduleSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State var date: Date
    let onConfirm: (Date) -> Void

    init(initial: Date, onConfirm: @escaping (Date) -> Void) {
        self._date = State(initialValue: initial)
        self.onConfirm = onConfirm
    }

    var body: some View {
        NavigationStack {
            Form {
                DatePicker("发送时间", selection: $date, in: Date()...)
            }
            .navigationTitle("定时发送")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("取消") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("确定") { onConfirm(date); dismiss() }
                }
            }
        }
        #if os(macOS)
        .frame(width: 380, height: 180)
        #endif
    }
}
