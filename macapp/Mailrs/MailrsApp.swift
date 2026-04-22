import SwiftUI

@main
struct MailrsApp: App {
    @State private var app = AppModel()
    @Environment(\.scenePhase) private var scenePhase
    @AppStorage(SettingsKey.theme) private var themeRaw: String = ThemeMode.system.rawValue

    private var colorScheme: ColorScheme? {
        switch ThemeMode(rawValue: themeRaw) ?? .system {
        case .system: return nil
        case .light: return .light
        case .dark: return .dark
        }
    }

    @State private var bgManager: BackgroundRefreshManager?

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(app)
                .preferredColorScheme(colorScheme)
                .task {
                    if bgManager == nil {
                        let m = BackgroundRefreshManager(appModel: app)
                        m.register()
                        bgManager = m
                    }
                }
                .onChange(of: scenePhase) { _, phase in
                    handleScenePhase(phase)
                }
        }
        #if os(macOS)
        .defaultSize(width: 1100, height: 720)
        #endif
    }

    private func handleScenePhase(_ phase: ScenePhase) {
        #if os(iOS)
        switch phase {
        case .active:
            Task { await app.resumeRealtime() }
        case .background:
            Task { await app.suspendRealtime() }
            bgManager?.scheduleNext()
        case .inactive:
            break
        @unknown default:
            break
        }
        #endif
    }
}
