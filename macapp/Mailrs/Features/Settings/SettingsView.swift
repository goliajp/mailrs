import SwiftUI

struct SettingsView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(AppModel.self) private var app

    @AppStorage(SettingsKey.theme) private var themeRaw: String = ThemeMode.system.rawValue
    @AppStorage(SettingsKey.notificationsEnabled) private var notificationsEnabled: Bool = true
    @AppStorage(SettingsKey.notificationSound) private var notificationSound: Bool = true
    @AppStorage(SettingsKey.signature) private var signature: String = ""
    @AppStorage(SettingsKey.signatureEnabled) private var signatureEnabled: Bool = false

    private var theme: Binding<ThemeMode> {
        Binding(
            get: { ThemeMode(rawValue: themeRaw) ?? .system },
            set: { themeRaw = $0.rawValue }
        )
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("外观") {
                    Picker("主题", selection: theme) {
                        ForEach(ThemeMode.allCases) { t in
                            Text(t.displayName).tag(t)
                        }
                    }
                }

                Section("通知") {
                    Toggle("新邮件通知", isOn: $notificationsEnabled)
                    Toggle("通知声音", isOn: $notificationSound).disabled(!notificationsEnabled)
                }

                Section("签名") {
                    Toggle("发件时附加签名", isOn: $signatureEnabled)
                    TextEditor(text: $signature)
                        .frame(minHeight: 80)
                        .disabled(!signatureEnabled)
                }

                Section("账户") {
                    if let info = app.authStore.authInfo {
                        LabeledContent("姓名", value: info.displayName)
                        LabeledContent("邮箱", value: info.address)
                        LabeledContent("域", value: info.accessibleDomains.joined(separator: ", "))
                    }
                    Button(role: .destructive) {
                        Task {
                            await app.signOut()
                            dismiss()
                        }
                    } label: {
                        Text("登出")
                    }
                }

                Section("关于") {
                    LabeledContent("版本", value: Self.versionString)
                    LabeledContent("API", value: AppConfig.apiBaseURL.absoluteString)
                }
            }
            .navigationTitle("设置")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("完成") { dismiss() }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 480, minHeight: 520)
        #endif
    }

    private static var versionString: String {
        let short = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "?"
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "?"
        return "\(short) (\(build))"
    }
}
