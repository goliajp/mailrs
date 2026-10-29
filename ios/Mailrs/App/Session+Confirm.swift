import Foundation

extension Session {
    /// Run `change`; when the server answers that it wants the password
    /// again, ask for it and run `change` once more.
    func confirmed<T>(_ change: () async throws -> T) async throws -> T {
        do {
            return try await change()
        } catch MailrsError.confirmRequired {
            guard await confirmGate.ask() else { throw MailrsError.confirmRequired }
            return try await change()
        }
    }

    /// The prompt's Continue.
    func submitConfirmation(password: String, code: String) async {
        guard let client, confirmGate.prompt != nil else { return }
        confirmGate.prompt?.busy = true
        confirmGate.prompt?.message = nil
        let sent = confirmGate.prompt?.needsCode == true ? code : nil
        do {
            try await client.confirm(password: password, code: sent)
            confirmGate.finish(confirmed: true)
        } catch MailrsError.totpRequired {
            confirmGate.prompt?.needsCode = true
            confirmGate.prompt?.busy = false
            confirmGate.prompt?.message = MailrsError.totpRequired.localizedDescription
        } catch let error as MailrsError {
            if case .confirmFailed = error {
                confirmGate.prompt?.busy = false
                confirmGate.prompt?.message = error.localizedDescription
                return
            }
            confirmGate.finish(confirmed: false)
            banner = error.localizedDescription
            if error == .sessionEnded { signOut() }
        } catch {
            confirmGate.finish(confirmed: false)
            banner = error.localizedDescription
        }
    }

    /// The prompt's Cancel: the change stays refused.
    func cancelConfirmation() {
        confirmGate.finish(confirmed: false)
    }
}
