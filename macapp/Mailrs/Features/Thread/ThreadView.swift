import SwiftUI

struct ThreadView: View {
    let threadId: String
    @Bindable var model: ThreadModel
    @Environment(AppModel.self) private var app

    @State private var showDeleteConfirm: Bool = false
    @State private var showSnoozeSheet: Bool = false

    var body: some View {
        Group {
            if model.isLoading && model.messages.isEmpty {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let err = model.errorMessage, model.messages.isEmpty {
                errorState(err)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 12) {
                        threadHeader
                        ForEach(model.messages) { message in
                            MessageBubbleView(
                                message: message,
                                attachmentService: app.attachmentService,
                                inviteService: app.inviteService
                            )
                        }
                    }
                    .padding()
                }
            }
        }
        .task(id: threadId) {
            await model.load(threadId: threadId)
        }
        .toolbar { toolbarContent }
        .confirmationDialog("删除整个线程？", isPresented: $showDeleteConfirm, titleVisibility: .visible) {
            Button("删除", role: .destructive) {
                Task {
                    _ = await model.delete()
                    await app.mailModel.refresh()
                    app.mailModel.selectedThreadId = nil
                }
            }
            Button("取消", role: .cancel) {}
        } message: {
            Text("删除操作不可撤销。")
        }
        .sheet(isPresented: $showSnoozeSheet) {
            SnoozeSheet { date in
                Task {
                    if await model.snooze(until: date) {
                        await app.mailModel.refresh()
                    }
                }
            }
        }
    }

    private var threadHeader: some View {
        VStack(alignment: .leading, spacing: 4) {
            let subject = model.messages.last?.subject ?? ""
            Text(subject.isEmpty ? "(无主题)" : subject)
                .font(.title3.bold())
                .textSelection(.enabled)
            HStack(spacing: 12) {
                Text("\(model.messages.count) 条消息")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let summary = model.messages.last?.summary, !summary.isEmpty {
                    Text(summary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
        }
    }

    private func errorState(_ message: String) -> some View {
        VStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: 44))
                .foregroundStyle(.orange)
            Text(message).multilineTextAlignment(.center).foregroundStyle(.secondary)
            Button("重试") {
                Task { await model.load(threadId: threadId) }
            }
            .buttonStyle(.bordered)
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            Button {
                if let m = model.messages.last, let tid = model.currentThreadId {
                    app.startReply(threadId: tid, message: m, replyAll: false)
                }
            } label: {
                Label("回复", systemImage: "arrowshape.turn.up.left")
            }
            .disabled(model.messages.isEmpty)

            Button {
                if let m = model.messages.last, let tid = model.currentThreadId {
                    app.startReply(threadId: tid, message: m, replyAll: true)
                }
            } label: {
                Label("回复全部", systemImage: "arrowshape.turn.up.left.2")
            }
            .disabled(model.messages.isEmpty)

            Button {
                if let m = model.messages.last, let tid = model.currentThreadId {
                    app.startForward(threadId: tid, message: m)
                }
            } label: {
                Label("转发", systemImage: "arrowshape.turn.up.right")
            }
            .disabled(model.messages.isEmpty)

            Button {
                Task {
                    if await model.markUnread() {
                        if let id = model.currentThreadId {
                            app.mailModel.markConversationUnread(threadId: id)
                        }
                    }
                }
            } label: {
                Label("标为未读", systemImage: "envelope.badge")
            }

            Button {
                let starred = currentFlagged
                Task {
                    if await model.toggleStar(currentlyStarred: starred),
                       let id = model.currentThreadId {
                        app.mailModel.setFlagged(threadId: id, flagged: !starred)
                    }
                }
            } label: {
                Label(currentFlagged ? "取消星标" : "星标",
                      systemImage: currentFlagged ? "star.fill" : "star")
            }
            .tint(currentFlagged ? .yellow : nil)

            Button {
                Task {
                    if await model.archive() {
                        await app.mailModel.refresh()
                        app.mailModel.selectedThreadId = nil
                    }
                }
            } label: {
                Label("归档", systemImage: "archivebox")
            }

            Menu {
                Button("1 小时后") {
                    Task { await snoozeFor(hours: 1) }
                }
                Button("明天早上 9 点") {
                    Task { await snoozeForTomorrowMorning() }
                }
                Button("下周一 9 点") {
                    Task { await snoozeForNextMondayMorning() }
                }
                Divider()
                Button("自定义…") { showSnoozeSheet = true }
            } label: {
                Label("稍后提醒", systemImage: "clock")
            }

            if let senderEmail = lastSenderEmail {
                Menu {
                    ForEach(FeedbackAction.allCases, id: \.self) { act in
                        Button {
                            Task { _ = await model.feedback(senderEmail: senderEmail, action: act) }
                        } label: {
                            Label(act.displayName, systemImage: act.systemImage)
                        }
                    }
                } label: {
                    Label("发件人操作", systemImage: "person.crop.circle.badge.questionmark")
                }
            }

            Button(role: .destructive) {
                showDeleteConfirm = true
            } label: {
                Label("删除", systemImage: "trash")
            }
        }
    }

    private var currentFlagged: Bool {
        guard let id = model.currentThreadId else { return false }
        return app.mailModel.conversations.first { $0.thread_id == id }?.flagged ?? false
    }

    private var lastSenderEmail: String? {
        guard let sender = model.messages.last?.sender else { return nil }
        return sender.extractedEmail
    }

    private func snoozeFor(hours: Int) async {
        let date = Calendar.current.date(byAdding: .hour, value: hours, to: .now) ?? .now
        if await model.snooze(until: date) {
            await app.mailModel.refresh()
        }
    }

    private func snoozeForTomorrowMorning() async {
        let cal = Calendar.current
        var comps = cal.dateComponents([.year, .month, .day], from: .now)
        comps.day = (comps.day ?? 0) + 1
        comps.hour = 9; comps.minute = 0
        if let date = cal.date(from: comps), await model.snooze(until: date) {
            await app.mailModel.refresh()
        }
    }

    private func snoozeForNextMondayMorning() async {
        let cal = Calendar.current
        let now = Date()
        let weekday = cal.component(.weekday, from: now)  // 1=Sun ... 7=Sat
        let daysUntilMon = (9 - weekday) % 7 == 0 ? 7 : (9 - weekday) % 7
        var comps = cal.dateComponents([.year, .month, .day], from: now)
        comps.day = (comps.day ?? 0) + daysUntilMon
        comps.hour = 9; comps.minute = 0
        if let date = cal.date(from: comps), await model.snooze(until: date) {
            await app.mailModel.refresh()
        }
    }
}

// MARK: snooze sheet

struct SnoozeSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State private var date: Date = Calendar.current.date(byAdding: .hour, value: 1, to: .now) ?? .now
    let onConfirm: (Date) -> Void

    var body: some View {
        NavigationStack {
            Form {
                DatePicker("提醒时间", selection: $date, in: Date()...)
            }
            .navigationTitle("稍后提醒")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("确定") {
                        onConfirm(date)
                        dismiss()
                    }
                }
            }
        }
        #if os(macOS)
        .frame(width: 360, height: 180)
        #endif
    }
}

// MARK: string helpers

extension String {
    var extractedEmail: String? {
        // "Name <email>" -> email ; else whole string if it looks like an email
        if let lt = firstIndex(of: "<"), let gt = firstIndex(of: ">"), lt < gt {
            return String(self[index(after: lt)..<gt])
        }
        if contains("@") { return trimmingCharacters(in: .whitespacesAndNewlines) }
        return nil
    }
}
