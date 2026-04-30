import Foundation
import Observation

@MainActor
@Observable
final class AppModel {
    let authStore: AuthStore
    let apiClient: ApiClient
    let authService: AuthService
    let conversationService: ConversationService
    let threadService: ThreadService
    let actionService: MessageActionService
    let attachmentService: AttachmentService
    let inviteService: InviteService
    let mailModel: MailModel
    let threadModel: ThreadModel
    let mailSendService: MailSendService
    let draftService: DraftService
    let eventsClient: EventsClient
    var activeComposeModel: ComposeModel?

    init() {
        let store = AuthStore()
        let api = ApiClient(baseURL: AppConfig.apiBaseURL, tokenProvider: store)
        let convService = ConversationService(api: api)
        let thrService = ThreadService(api: api)
        let actService = MessageActionService(api: api)
        let sendSvc = MailSendService(api: api)
        let draftSvc = DraftService(api: api)
        self.authStore = store
        self.apiClient = api
        self.authService = AuthService(api: api)
        self.conversationService = convService
        self.threadService = thrService
        self.actionService = actService
        self.mailSendService = sendSvc
        self.draftService = draftSvc
        self.attachmentService = AttachmentService(baseURL: AppConfig.apiBaseURL, tokenProvider: store)
        self.inviteService = InviteService(api: api)
        self.mailModel = MailModel(service: convService)
        self.threadModel = ThreadModel(threadService: thrService, actionService: actService)
        self.eventsClient = EventsClient()
    }

    // MARK: compose entry points

    func startNewCompose() {
        let from = authStore.authInfo?.address ?? ""
        activeComposeModel = ComposeModel(
            fromAddress: from,
            sendService: mailSendService,
            draftService: draftService
        )
    }

    func startReply(threadId: String, message: ThreadMessage, replyAll: Bool) {
        let from = authStore.authInfo?.address ?? ""
        let model = ComposeModel(
            fromAddress: from,
            sendService: mailSendService,
            draftService: draftService
        )
        model.prepareReply(to: message, threadId: threadId, replyAll: replyAll)
        activeComposeModel = model
    }

    func startForward(threadId: String, message: ThreadMessage) {
        let from = authStore.authInfo?.address ?? ""
        let model = ComposeModel(
            fromAddress: from,
            sendService: mailSendService,
            draftService: draftService
        )
        model.prepareForward(from: message, threadId: threadId)
        activeComposeModel = model
    }

    func closeCompose() {
        activeComposeModel = nil
    }

    func bootstrap() async {
        await authStore.restore()

        if authStore.isAuthenticated, let token = authStore.authInfo?.token {
            do {
                let refreshed = try await authService.me(currentToken: token)
                try authStore.persist(refreshed)
            } catch ApiError.unauthorized {
                // AuthStore already cleared by the 401 handler.
            } catch {
                // Transient failure; keep the cached auth info and retry on next app launch.
            }
        }

        NotificationManager.shared.configure()
        NotificationManager.shared.onSelectThread = { [weak self] tid in
            self?.mailModel.selectedThreadId = tid
        }

        if authStore.isAuthenticated { await startRealtime() }
    }

    func signOut() async {
        await stopRealtime()
        if authStore.isAuthenticated {
            try? await authService.logout()
        }
        authStore.signOut()
    }

    // MARK: realtime lifecycle

    func startRealtime() async {
        guard let auth = authStore.authInfo else { return }
        mailModel.attachRealtime(
            client: eventsClient,
            threadModel: threadModel,
            userAddress: auth.address
        )
        await eventsClient.start(token: auth.token)
    }

    func stopRealtime() async {
        await eventsClient.stop()
        mailModel.detachRealtime()
    }

    func suspendRealtime() async {
        await eventsClient.suspend()
    }

    func resumeRealtime() async {
        await eventsClient.resume()
    }
}
