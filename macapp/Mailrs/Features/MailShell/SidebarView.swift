import SwiftUI

struct SidebarView: View {
    @Environment(AppModel.self) private var app
    @Bindable var mail: MailModel
    @Binding var showSettings: Bool

    var body: some View {
        List(selection: folderSelection) {
            Section("文件夹") {
                ForEach(MailFolder.allCases) { folder in
                    folderRow(folder)
                        .tag(folder)
                }
            }
            Section("分类") {
                categoryRow(nil, label: "全部", systemImage: "square.grid.2x2")
                ForEach(MailCategory.allCases) { cat in
                    categoryRow(cat, label: cat.displayName, systemImage: cat.systemImage)
                }
            }
        }
        #if os(macOS)
        .listStyle(.sidebar)
        #else
        .listStyle(.insetGrouped)
        #endif
        .safeAreaInset(edge: .bottom) {
            footer
        }
    }

    private var folderSelection: Binding<MailFolder?> {
        Binding(
            get: { mail.folder },
            set: { newValue in
                if let newValue { mail.folder = newValue }
            }
        )
    }

    private func folderRow(_ folder: MailFolder) -> some View {
        HStack {
            Label(folder.displayName, systemImage: folder.systemImage)
            Spacer()
            if folder == .inbox && mail.totalUnread > 0 {
                Text("\(mail.totalUnread)")
                    .font(.caption.monospacedDigit())
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.secondary.opacity(0.2))
                    .clipShape(Capsule())
            }
        }
    }

    private var connectionDot: some View {
        let (color, tooltip): (Color, String) = {
            switch mail.connectionStatus {
            case .connected: return (.green, "实时已连接")
            case .connecting: return (.orange, "连接中")
            case .offline: return (.gray, "离线")
            }
        }()
        return Circle()
            .fill(color)
            .frame(width: 6, height: 6)
            .help(tooltip)
    }

    private func categoryRow(_ category: MailCategory?, label: String, systemImage: String) -> some View {
        Button {
            mail.category = category
        } label: {
            HStack {
                Label(label, systemImage: systemImage)
                Spacer()
                if mail.category == category {
                    Image(systemName: "checkmark")
                        .font(.caption)
                        .foregroundStyle(.tint)
                }
            }
        }
        .buttonStyle(.plain)
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 8) {
            Divider()
            if let info = app.authStore.authInfo {
                HStack(spacing: 8) {
                    Circle()
                        .fill(.tint.opacity(0.2))
                        .frame(width: 28, height: 28)
                        .overlay(
                            Text(String(info.displayName.prefix(1)))
                                .font(.caption.bold())
                                .foregroundStyle(.tint)
                        )
                    VStack(alignment: .leading, spacing: 1) {
                        HStack(spacing: 4) {
                            Text(info.displayName).font(.caption.bold()).lineLimit(1)
                            connectionDot
                        }
                        Text(info.address).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer()
                    Menu {
                        Button {
                            showSettings = true
                        } label: {
                            Label("设置…", systemImage: "gearshape")
                        }
                        Divider()
                        Button(role: .destructive) {
                            Task { await app.signOut() }
                        } label: {
                            Label("登出", systemImage: "rectangle.portrait.and.arrow.right")
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                }
                .padding(.horizontal, 12)
                .padding(.bottom, 8)
            }
        }
    }
}
