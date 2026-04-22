import SwiftUI

struct MailShellView: View {
    @Environment(AppModel.self) private var app
    @State private var columnVisibility: NavigationSplitViewVisibility = .automatic
    @State private var showSettings: Bool = false

    var body: some View {
        @Bindable var app = app
        NavigationSplitView(columnVisibility: $columnVisibility) {
            SidebarView(mail: app.mailModel, showSettings: $showSettings)
                .navigationSplitViewColumnWidth(min: 200, ideal: 220, max: 280)
        } content: {
            ConversationListView(model: app.mailModel)
                .navigationSplitViewColumnWidth(min: 320, ideal: 380, max: 520)
        } detail: {
            if let threadId = app.mailModel.selectedThreadId {
                ThreadView(threadId: threadId, model: app.threadModel)
            } else {
                EmptyThreadPane(onCompose: { app.startNewCompose() })
            }
        }
        .task {
            if app.mailModel.conversations.isEmpty {
                await app.mailModel.refresh()
            }
        }
        .sheet(isPresented: $showSettings) {
            SettingsView()
        }
        .sheet(
            isPresented: Binding(
                get: { app.activeComposeModel != nil },
                set: { if !$0 { app.closeCompose() } }
            )
        ) {
            if let model = app.activeComposeModel {
                ComposeView(model: model) {
                    Task { await app.mailModel.refresh() }
                }
            }
        }
    }
}

struct EmptyThreadPane: View {
    let onCompose: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "envelope.open")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("选择一封邮件")
                .foregroundStyle(.secondary)
            Button {
                onCompose()
            } label: {
                Label("写新邮件", systemImage: "square.and.pencil")
            }
            .buttonStyle(.bordered)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
