import Foundation

extension Session {
    /// Set a new password with a reset code. Nobody is signed in: the
    /// person signs in afterwards with the new password.
    func resetPassword(address: String, code: String, newPassword: String) async throws {
        try await MailrsClient(baseURL: baseURL)
            .resetPassword(address: address, code: code, newPassword: newPassword)
    }
}
