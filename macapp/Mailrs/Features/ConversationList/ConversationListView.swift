import SwiftUI

struct ConversationListView: View {
    @Bindable var model: MailModel
    @Environment(AppModel.self) private var app
    @State private var pendingSearch: String = ""

    var body: some View {
        VStack(spacing: 0) {
            filterBar
            Divider()
            listBody
        }
        .navigationTitle(title)
        #if os(macOS)
        .navigationSubtitle(model.searchQuery.isEmpty ? "" : "搜索: \(model.searchQuery)")
        #endif
        .toolbar { toolbarContent }
        .searchable(text: $pendingSearch, prompt: "搜索邮件")
        .onSubmit(of: .search) {
            model.searchQuery = pendingSearch
            Task { await model.runSearch() }
        }
        .onChange(of: pendingSearch) { _, newValue in
            if newValue.isEmpty && !model.searchQuery.isEmpty {
                Task {
                    model.searchQuery = ""
                    await model.refresh()
                }
            }
        }
    }

    private var title: String {
        if let cat = model.category { return cat.displayName }
        return model.folder.displayName
    }

    private var filterBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(QuickFilter.allCases) { f in
                    chip(label: f.displayName, systemImage: f.systemImage, selected: model.quickFilter == f) {
                        model.quickFilter = f
                    }
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
    }

    private func chip(label: String, systemImage: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 4) {
                Image(systemName: systemImage).font(.caption)
                Text(label).font(.caption)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(selected ? Color.accentColor.opacity(0.2) : Color.secondary.opacity(0.1))
            .foregroundStyle(selected ? Color.accentColor : Color.primary)
            .clipShape(Capsule())
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private var listBody: some View {
        if model.isInitialLoading && model.conversations.isEmpty {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let err = model.errorMessage, model.conversations.isEmpty {
            errorState(err)
        } else if model.visibleConversations.isEmpty {
            emptyState
        } else {
            List(selection: $model.selectedThreadId) {
                ForEach(model.visibleConversations) { conv in
                    ConversationRow(conversation: conv)
                        .tag(conv.thread_id)
                        .task { await model.loadMoreIfNeeded(for: conv) }
                }
                if model.isLoadingMore {
                    HStack {
                        Spacer()
                        ProgressView().controlSize(.small)
                        Spacer()
                    }
                    .listRowSeparator(.hidden)
                }
            }
            .listStyle(.plain)
            .refreshable { await model.refresh() }
        }
    }

    @ViewBuilder
    private var emptyState: some View {
        VStack(spacing: 8) {
            Image(systemName: "envelope")
                .font(.system(size: 44))
                .foregroundStyle(.secondary)
            Text(model.searchQuery.isEmpty ? "这里没有邮件" : "无匹配")
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func errorState(_ message: String) -> some View {
        VStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: 44))
                .foregroundStyle(.orange)
            Text(message)
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button("重试") {
                Task { await model.refresh() }
            }
            .buttonStyle(.bordered)
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            Button {
                app.startNewCompose()
            } label: {
                Label("写邮件", systemImage: "square.and.pencil")
            }
        }
        ToolbarItem(placement: .primaryAction) {
            Menu {
                Picker("排序", selection: $model.sortOrder) {
                    ForEach(SortOrder.allCases) { order in
                        Text(order.displayName).tag(order)
                    }
                }
            } label: {
                Label("排序", systemImage: "arrow.up.arrow.down")
            }
        }
        ToolbarItem(placement: .primaryAction) {
            Button {
                Task { await model.refresh() }
            } label: {
                Label("刷新", systemImage: "arrow.clockwise")
            }
            .disabled(model.isInitialLoading)
        }
    }
}
