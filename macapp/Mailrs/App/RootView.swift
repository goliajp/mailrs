import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var app
    @State private var hasBootstrapped = false

    var body: some View {
        Group {
            if !hasBootstrapped {
                ProgressView("初始化…")
                    .task {
                        await app.bootstrap()
                        hasBootstrapped = true
                    }
            } else if app.authStore.isAuthenticated {
                MailShellView()
            } else {
                LoginView(model: LoginModel(
                    authService: app.authService,
                    authStore: app.authStore
                ))
            }
        }
        .onChange(of: app.authStore.isAuthenticated) { _, authenticated in
            Task {
                if authenticated {
                    await app.startRealtime()
                } else {
                    await app.stopRealtime()
                }
            }
        }
    }
}
