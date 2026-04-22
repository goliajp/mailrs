import Foundation
import Observation

@MainActor
@Observable
final class LoginModel {
    enum Stage: Equatable {
        case credentials
        case totp
    }

    var email: String = ""
    var password: String = ""
    var totpCode: String = ""
    var stage: Stage = .credentials
    var isSubmitting: Bool = false
    var errorMessage: String?

    private let authService: AuthService
    private let authStore: AuthStore

    init(authService: AuthService, authStore: AuthStore) {
        self.authService = authService
        self.authStore = authStore
        self.email = authStore.rememberedEmail ?? ""
    }

    var canSubmit: Bool {
        guard !isSubmitting else { return false }
        switch stage {
        case .credentials: return !email.isEmpty && !password.isEmpty
        case .totp: return totpCode.count >= 6
        }
    }

    func submit() async {
        guard canSubmit else { return }
        isSubmitting = true
        errorMessage = nil
        defer { isSubmitting = false }

        do {
            let outcome = try await authService.login(
                address: email.trimmingCharacters(in: .whitespaces),
                password: password,
                totpCode: stage == .totp ? totpCode : nil
            )
            switch outcome {
            case .totpRequired:
                stage = .totp
                totpCode = ""
            case .success(let info):
                try authStore.persist(info)
                password = ""
                totpCode = ""
            }
        } catch let err as ApiError {
            errorMessage = err.errorDescription
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func backToCredentials() {
        stage = .credentials
        totpCode = ""
        errorMessage = nil
    }
}
