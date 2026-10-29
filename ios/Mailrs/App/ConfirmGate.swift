import Foundation

/// The password prompt a guarded change waits on.
///
/// The server refuses some changes — an alias, a group member, an account,
/// an agent key — with `confirm_required` until this session has typed its
/// password in the last ten minutes. `ask` shows the prompt and suspends
/// until the person confirms or gives up; changes refused at the same
/// moment wait on the same prompt.
@Observable
@MainActor
final class ConfirmGate {
    struct Prompt: Equatable {
        var needsCode = false
        var message: String?
        var busy = false
    }

    var prompt: Prompt?
    private var waiting: [CheckedContinuation<Bool, Never>] = []

    func ask() async -> Bool {
        await withCheckedContinuation { done in
            waiting.append(done)
            if prompt == nil { prompt = Prompt() }
        }
    }

    func finish(confirmed: Bool) {
        let all = waiting
        waiting = []
        prompt = nil
        for done in all { done.resume(returning: confirmed) }
    }
}
