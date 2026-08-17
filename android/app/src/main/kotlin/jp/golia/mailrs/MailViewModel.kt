package jp.golia.mailrs

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.Admin
import jp.golia.mailrs.wire.ContentUriBody
import jp.golia.mailrs.wire.MailList
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.RecipientAutocomplete
import jp.golia.mailrs.wire.ShareIntent
import jp.golia.mailrs.wire.ReplyRecipients
import jp.golia.mailrs.wire.TokenStore
import jp.golia.mailrs.wire.Wire
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * The app's state, in one place.
 *
 * There is exactly one copy of every fact here. The web client learned
 * that the hard way — `.claude/rules/frontend/no-rq-mirror.md` — where a
 * component kept its own `useState` mirror of the query cache and the
 * two diverged on every A→B→A navigation. A second copy of "which
 * thread is open" or "what messages it has" would do the same thing
 * here, so the screens read this and hold nothing.
 */
class MailViewModel(app: Application) : AndroidViewModel(app) {

    private val client = MailrsClient(TokenStore(app))
    private val prefs = Prefs(app)
    private var nextDraftId = 1
    private var pending: PendingTriage? = null
    private var undoToken = 0
    private var searchToken = 0
    private var contactToken = 0

    /**
     * Point the app at a stub, the way the iOS suite's
     * `-mailrsBaseURL` launch argument does.
     *
     * Debug builds only — `BuildConfig.ALLOW_SERVER_OVERRIDE`. A shipped
     * app that took its server from an intent would let any app on the
     * phone redirect someone's mail and password to a host of its
     * choosing, which is a credential-phishing primitive rather than a
     * testing convenience.
     */
    fun useServer(url: String?) {
        if (!BuildConfig.ALLOW_SERVER_OVERRIDE) return
        client.baseUrlOverride = url
    }

    private val _state = MutableStateFlow(
        UiState(
            signedIn = client.session != null,
            server = client.session?.server.orEmpty(),
            appearance = prefs.appearance,
        )
    )
    val state: StateFlow<UiState> = _state.asStateFlow()

    init {
        if (client.session != null) refresh()
    }

    fun signIn(server: String, username: String, password: String) {
        _state.value = _state.value.copy(busy = true, error = null)
        viewModelScope.launch {
            when (val r = client.login(server, username, password)) {
                is MailrsClient.Outcome.Ok -> {
                    _state.value = _state.value.copy(
                        signedIn = true,
                        busy = false,
                        server = client.session?.server.orEmpty(),
                    )
                    refresh()
                }
                is MailrsClient.Outcome.Err ->
                    _state.value = _state.value.copy(busy = false, error = r.message)
            }
        }
    }

    fun signOut() {
        client.signOut()
        _state.value = UiState()
    }

    fun refresh() {
        _state.value = _state.value.copy(busy = true, error = null)
        viewModelScope.launch {
            when (val r = client.conversations(_state.value.list)) {
                is MailrsClient.Outcome.Ok ->
                    _state.value = _state.value.copy(busy = false, conversations = r.value)
                is MailrsClient.Outcome.Err ->
                    _state.value = _state.value.copy(busy = false, error = r.message)
            }
        }
    }

    fun open(conversation: Wire.Conversation) {
        _state.value = _state.value.copy(open = conversation, messages = emptyList(), busy = true, error = null)
        viewModelScope.launch {
            when (val r = client.thread(conversation.threadId)) {
                is MailrsClient.Outcome.Ok -> {
                    _state.value = _state.value.copy(busy = false, messages = r.value)
                    if (conversation.unreadCount > 0) {
                        client.markRead(conversation.threadId)
                        // Reflect it locally rather than refetching the
                        // whole list for one counter.
                        _state.value = _state.value.copy(
                            conversations = _state.value.conversations.map {
                                if (it.threadId == conversation.threadId) it.copy(unreadCount = 0) else it
                            }
                        )
                    }
                }
                is MailrsClient.Outcome.Err ->
                    _state.value = _state.value.copy(busy = false, error = r.message)
            }
        }
    }

    /**
     * Start a message. `replyTo` null is a new one.
     *
     * The reply's recipients, subject and quoted history come from
     * `ReplyRecipients`, which is the web's rules ported once and
     * unit-tested — not re-derived here, where a dropped cc would be
     * invisible until somebody noticed a colleague missing from a
     * thread.
     */
    fun compose(replyTo: Wire.Message? = null, all: Boolean = false) {
        val draft = if (replyTo == null) {
            Draft(id = nextDraftId++)
        } else {
            val me = _state.value.myAddress
            Draft(
                id = nextDraftId++,
                to = if (all) {
                    ReplyRecipients.replyAll(replyTo.sender, replyTo.recipients, me).joinToString(", ")
                } else {
                    ReplyRecipients.reply(replyTo.sender).joinToString(", ")
                },
                subject = ReplyRecipients.subject(replyTo.subject),
                body = ReplyRecipients.quote(
                    replyTo.sender,
                    replyTo.internalDate,
                    replyTo.textBody.orEmpty(),
                ),
                inReplyTo = replyTo.messageId,
                replyToThreadId = _state.value.open?.threadId,
            )
        }
        _state.value = _state.value.copy(composing = draft, error = null)
    }

    /**
     * Take on a picked file.
     *
     * The name and size come from the content resolver rather than the
     * URI's last path segment: a document provider's URI is an opaque
     * id, and "msf:1000000042" is not a filename anybody wants to see
     * arrive in their mail.
     */
    fun attach(uris: List<android.net.Uri>) {
        val draft = _state.value.composing ?: return
        val resolver = getApplication<Application>().contentResolver
        val added = uris.mapNotNull { uri ->
            runCatching {
                resolver.query(uri, null, null, null, null)?.use { cursor ->
                    val nameAt = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                    val sizeAt = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
                    if (!cursor.moveToFirst()) return@use null
                    Attached(
                        uri = uri,
                        filename = if (nameAt >= 0) cursor.getString(nameAt) else "attachment",
                        size = if (sizeAt >= 0 && !cursor.isNull(sizeAt)) cursor.getLong(sizeAt) else 0,
                    )
                }
            }.getOrNull()
        }
        if (added.isEmpty()) return
        _state.value = _state.value.copy(
            composing = draft.copy(attachments = draft.attachments + added),
        )
    }

    fun detach(a: Attached) {
        val draft = _state.value.composing ?: return
        _state.value = _state.value.copy(
            composing = draft.copy(attachments = draft.attachments.filterNot { it.uri == a.uri }),
        )
    }

    /**
     * Start a message another app asked for.
     *
     * `mailto:` from a link, or the share sheet with text and files.
     * Anything that arrives goes into a draft, so leaving still saves
     * it — a shared photo that vanished because the person had second
     * thoughts about the recipient would be worse than no share target
     * at all.
     *
     * Signed out, this does nothing but leave a draft waiting: the
     * sign-in screen shows first, and the composer is there afterwards.
     */
    fun composeFromShare(
        mailto: ShareIntent.Mailto? = null,
        subject: String = "",
        body: String = "",
        attachments: List<android.net.Uri> = emptyList(),
    ) {
        val draft = Draft(
            id = nextDraftId++,
            to = mailto?.to.orEmpty(),
            cc = mailto?.cc.orEmpty(),
            bcc = mailto?.bcc.orEmpty(),
            subject = mailto?.subject?.takeIf(String::isNotBlank) ?: subject,
            body = mailto?.body?.takeIf(String::isNotBlank) ?: body,
        )
        _state.value = _state.value.copy(composing = draft, error = null)
        if (attachments.isNotEmpty()) attach(attachments)
    }

    /** Every keystroke, straight into the one copy of the draft. */
    fun editDraft(
        to: String? = null,
        cc: String? = null,
        bcc: String? = null,
        subject: String? = null,
        body: String? = null,
    ) {
        val draft = _state.value.composing ?: return
        _state.value = _state.value.copy(
            composing = draft.copy(
                to = to ?: draft.to,
                cc = cc ?: draft.cc,
                bcc = bcc ?: draft.bcc,
                subject = subject ?: draft.subject,
                body = body ?: draft.body,
            ),
        )
    }

    /**
     * Leave the composer, keeping what was written.
     *
     * **Leaving saves.** Every mail client on the phone does, and a
     * half-written message is the one thing a person cannot get back by
     * trying again. An empty composer is not saved, or the drafts list
     * fills with blanks from every mis-tapped compose button.
     */
    fun cancelCompose() {
        val draft = _state.value.composing
        _state.value = _state.value.copy(composing = null, error = null)
        if (draft == null || draft.isEmpty) return
        viewModelScope.launch {
            val saved = client.saveDraft(
                Wire.SaveDraftRequest(
                    id = draft.serverId,
                    to = draft.to,
                    cc = draft.cc,
                    bcc = draft.bcc,
                    subject = draft.subject,
                    body = draft.body,
                    replyToThreadId = draft.replyToThreadId,
                ),
            )
            if (saved is MailrsClient.Outcome.Ok) {
                _state.value = _state.value.copy(draftSaved = true)
            }
        }
    }

    fun draftNoticeShown() {
        _state.value = _state.value.copy(draftSaved = false)
    }

    /** Load the drafts list, and open one for editing. */
    fun openDrafts() {
        _state.value = _state.value.copy(draftsOpen = true, busy = true, error = null)
        viewModelScope.launch {
            _state.value = when (val r = client.drafts()) {
                is MailrsClient.Outcome.Ok ->
                    _state.value.copy(busy = false, drafts = r.value.sortedByDescending { it.updatedAt })
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
        }
    }

    fun closeDrafts() {
        _state.value = _state.value.copy(draftsOpen = false)
    }

    /**
     * Reopen a saved draft.
     *
     * Its server id travels with it, so saving again updates the same
     * row rather than leaving a copy behind on every edit.
     */
    fun editSavedDraft(d: Wire.Draft) {
        _state.value = _state.value.copy(
            draftsOpen = false,
            composing = Draft(
                id = nextDraftId++,
                to = d.to,
                cc = d.cc,
                bcc = d.bcc,
                subject = d.subject,
                body = d.body,
                replyToThreadId = d.replyToThreadId,
                serverId = d.id,
            ),
        )
    }

    fun discardDraft(d: Wire.Draft) {
        _state.value = _state.value.copy(drafts = _state.value.drafts.filterNot { it.id == d.id })
        viewModelScope.launch { client.deleteDraft(d.id) }
    }

    /**
     * Who a To line names, once the commas and the whitespace are gone.
     *
     * One definition, used by both the send button's enabled state and
     * the send itself. They were two — `to.isNotBlank()` on the button
     * and this on the send — and `"   "` satisfied the first while
     * failing the second, so the button stayed live and the message
     * that explains why never appeared. Two rules for one question is
     * how a control ends up doing nothing with no explanation.
     */
    fun recipientsIn(to: String): List<String> =
        to.split(',', ';').map { it.trim() }.filter { it.isNotEmpty() }

    fun send() {
        val draft = _state.value.composing ?: return
        val recipients = recipientsIn(draft.to)
        if (recipients.isEmpty()) {
            _state.value = _state.value.copy(error = "A message needs somebody to go to.")
            return
        }
        _state.value = _state.value.copy(sending = true, error = null)
        viewModelScope.launch {
            val resolver = getApplication<Application>().contentResolver
            val r = if (draft.attachments.isEmpty()) {
                client.send(
                    recipients,
                    draft.subject,
                    draft.body,
                    draft.inReplyTo,
                    cc = recipientsIn(draft.cc),
                    bcc = recipientsIn(draft.bcc),
                )
            } else {
                client.sendMultipart(
                    to = recipients,
                    cc = recipientsIn(draft.cc),
                    bcc = recipientsIn(draft.bcc),
                    subject = draft.subject,
                    body = draft.body,
                    inReplyTo = draft.inReplyTo,
                    attachments = draft.attachments.map { a ->
                        MailrsClient.Upload(a.filename, ContentUriBody(resolver, a.uri))
                    },
                )
            }
            when (r) {
                is MailrsClient.Outcome.Ok -> {
                    _state.value = _state.value.copy(sending = false, composing = null, sent = true)
                    refresh()
                }
                is MailrsClient.Outcome.Err ->
                    // The composer stays open. A send that failed and
                    // closed the screen would take the text with it,
                    // which is the one thing a person cannot get back.
                    _state.value = _state.value.copy(sending = false, error = r.message)
            }
        }
    }

    fun acknowledgeSent() {
        _state.value = _state.value.copy(sent = false)
    }

    /**
     * Triage a row: take it off the list now, tell the server later.
     *
     * **The row leaves immediately and the request waits.** Undo is the
     * protection this screen offers instead of a confirmation — the
     * design doc's rule, and Gmail's — so a request sent at the swipe
     * would have to be undone by a second request, and a person who
     * swipes four rows and undoes one would have paid for eight round
     * trips to move three.
     *
     * The pending action is held for [UNDO_WINDOW_MS]. Swiping another
     * row commits the previous one rather than replacing it, because a
     * queue of undoable things is a queue of things a person cannot
     * reason about — one is what a snackbar can honestly offer.
     */
    fun triage(conversation: Wire.Conversation, verb: MailrsClient.Verb) {
        pending?.let { commit(it) }
        val remaining = _state.value.conversations.filterNot { it.threadId == conversation.threadId }
        val action = PendingTriage(conversation, verb, _state.value.conversations)
        pending = action
        _state.value = _state.value.copy(conversations = remaining, undo = action)

        val token = ++undoToken
        viewModelScope.launch {
            kotlinx.coroutines.delay(UNDO_WINDOW_MS)
            // A later swipe, or an undo, moved on without us.
            if (token != undoToken) return@launch
            pending?.let { commit(it) }
        }
    }

    /**
     * Fetch an attachment and put it somewhere another app can read.
     *
     * The bytes land in the cache under a per-message directory, so two
     * messages carrying `invoice.pdf` cannot overwrite each other's —
     * the filename is the sender's and is not unique. A `FileProvider`
     * URI is what leaves this app: handing out a `file://` path has
     * been an error since API 24, and a world-readable copy would be a
     * worse way to make it work.
     */
    fun openAttachment(uid: Int, index: Int, att: Wire.Attachment) {
        _state.value = _state.value.copy(openingAttachment = index, error = null)
        viewModelScope.launch {
            _state.value = when (val r = client.attachment(uid, index)) {
                is MailrsClient.Outcome.Ok -> {
                    val file = runCatching { writeToCache(uid, index, att.filename, r.value) }
                    file.fold(
                        onSuccess = {
                            _state.value.copy(
                                openingAttachment = null,
                                openFile = OpenedFile(it, att.contentType, att.filename),
                            )
                        },
                        onFailure = {
                            _state.value.copy(
                                openingAttachment = null,
                                error = "Could not save ${att.filename}: ${it.message}",
                            )
                        },
                    )
                }
                is MailrsClient.Outcome.Err ->
                    _state.value.copy(openingAttachment = null, error = r.message)
            }
        }
    }

    /**
     * Ask the server to leave the list.
     *
     * Only ever the one-click case, and only ever by the message's
     * identity — the advertised URLs identify the subscriber, so the
     * server takes the URL out of the message's own header rather than
     * being told where to post.
     *
     * A refusal is kept per message and shown, because an unsubscribe
     * that failed and looks like one that worked is how people end up
     * tapping it every week for a year.
     */
    fun unsubscribe(threadId: String, uid: Int) {
        _state.value = _state.value.copy(
            unsubscribing = _state.value.unsubscribing + (uid to Unsubscribing.Working),
        )
        viewModelScope.launch {
            val outcome = client.unsubscribe(threadId, uid)
            val verdict = when {
                outcome is MailrsClient.Outcome.Ok && outcome.value.ok -> Unsubscribing.Done
                else -> Unsubscribing.Failed
            }
            _state.value = _state.value.copy(
                unsubscribing = _state.value.unsubscribing + (uid to verdict),
            )
        }
    }

    /** The screen has handed it to another app; stop offering it. */
    fun attachmentOpened() {
        _state.value = _state.value.copy(openFile = null)
    }

    private fun writeToCache(uid: Int, index: Int, filename: String, bytes: ByteArray): java.io.File {
        val dir = java.io.File(getApplication<Application>().cacheDir, "attachments/$uid-$index")
        dir.mkdirs()
        // The sender chose this name. A name that walks out of the
        // directory — "../../databases/x" — must land in the directory
        // anyway, so only the last path component is kept.
        val safe = filename.substringAfterLast('/').ifBlank { "attachment" }
        val file = java.io.File(dir, safe)
        file.writeBytes(bytes)
        return file
    }

    /**
     * Search, with the server's ranking left alone.
     *
     * An empty term is not a search — it clears back to the inbox rather
     * than asking the server to rank everything.
     */
    fun search(term: String) {
        searchToken++
        val token = searchToken
        if (term.isBlank()) {
            _state.value = _state.value.copy(searchTerm = "", results = null, searching = false)
            return
        }
        _state.value = _state.value.copy(searchTerm = term, searching = true, error = null)
        viewModelScope.launch {
            val r = client.search(term, _state.value.list)
            // A slower earlier search must not overwrite a later one —
            // typing "ref" then "ref 2026" would otherwise settle on
            // whichever request the network happened to finish last.
            if (token != searchToken) return@launch
            _state.value = when (r) {
                is MailrsClient.Outcome.Ok ->
                    _state.value.copy(results = r.value, searching = false)
                is MailrsClient.Outcome.Err ->
                    _state.value.copy(searching = false, error = r.message)
            }
        }
    }

    /**
     * Selection mode: long-press starts it, tapping adds and removes.
     *
     * Android's own pattern for acting on many rows, and the reason the
     * row tap has two meanings — while a selection is on, tapping a row
     * changes the selection rather than opening it, which is what every
     * list on the phone does and what a reader will expect after the
     * first long press.
     */
    fun toggleSelected(threadId: String) {
        val now = _state.value.selected
        val next = if (threadId in now) now - threadId else now + threadId
        _state.value = _state.value.copy(selected = next)
    }

    fun clearSelection() {
        if (_state.value.selected.isEmpty()) return
        _state.value = _state.value.copy(selected = emptySet())
    }

    /**
     * Apply a verb to everything selected, in one request.
     *
     * The rows leave at once and the selection ends, because a
     * selection that survived its own action would invite the same
     * action twice. Unlike a swipe there is no undo window: the
     * snackbar can honestly offer one undo, and a bulk action that
     * silently deferred would leave the list disagreeing with the
     * server for five seconds while the reader watches.
     */
    fun applyToSelection(verb: MailrsClient.Verb) {
        val ids = _state.value.selected.toList()
        if (ids.isEmpty()) return
        val before = _state.value.conversations
        // Read and star leave the rows where they are; the rest take
        // them out of this list. Saying which is which here keeps the
        // list honest about what the server was asked to do.
        val staysInPlace = verb == MailrsClient.Verb.Read || verb == MailrsClient.Verb.Unread ||
            verb == MailrsClient.Verb.Star || verb == MailrsClient.Verb.Unstar
        _state.value = _state.value.copy(
            selected = emptySet(),
            conversations = if (staysInPlace) before else before.filterNot { it.threadId in ids },
        )
        viewModelScope.launch {
            when (val r = client.batch(verb, ids)) {
                is MailrsClient.Outcome.Ok -> if (staysInPlace) refresh()
                is MailrsClient.Outcome.Err ->
                    // It did not happen, so the rows come back. Mail
                    // that vanished on a failed request is mail the
                    // person believes they filed and did not.
                    _state.value = _state.value.copy(conversations = before, error = r.message)
            }
        }
    }

    /**
     * Show another list.
     *
     * The rows on screen belong to the list that was showing, so they
     * are cleared rather than left under a new heading — a moment of
     * Junk labelled Inbox is worse than a moment of nothing.
     */
    fun show(list: MailList) {
        if (list == _state.value.list) return
        searchToken++
        _state.value = _state.value.copy(
            list = list,
            selected = emptySet(),
            conversations = emptyList(),
            searchTerm = "",
            results = null,
            searching = false,
            error = null,
        )
        refresh()
    }

    /**
     * Contacts for the name being typed.
     *
     * Asked per field, so a suggestion for Cc cannot land in To. The
     * token rule is `RecipientAutocomplete`'s and the matching is the
     * server's — matching again here would be a second answer to one
     * question.
     */
    fun suggestContacts(field: RecipientField, line: String) {
        val token = RecipientAutocomplete.currentToken(line)
        if (!RecipientAutocomplete.shouldSuggest(token)) {
            _state.value = _state.value.copy(suggestions = emptyList(), suggestingFor = null)
            return
        }
        contactToken++
        val mine = contactToken
        viewModelScope.launch {
            val r = client.contacts(token)
            if (mine != contactToken) return@launch
            _state.value = when (r) {
                is MailrsClient.Outcome.Ok ->
                    _state.value.copy(suggestions = r.value, suggestingFor = field)
                // A suggestion list that cannot be fetched is not an
                // error worth a banner: the person can type the address.
                is MailrsClient.Outcome.Err ->
                    _state.value.copy(suggestions = emptyList(), suggestingFor = null)
            }
        }
    }

    fun clearSuggestions() {
        if (_state.value.suggestions.isEmpty()) return
        _state.value = _state.value.copy(suggestions = emptyList(), suggestingFor = null)
    }

    /**
     * Open an operator list and fetch it.
     *
     * Fetched every time rather than cached: these are answers about
     * what the server is configured to do right now, and a stale one
     * read as current is how an operator concludes a change did not
     * take.
     */
    fun openAdmin(section: AdminSection) {
        _state.value = _state.value.copy(adminOpen = section, busy = true, error = null)
        viewModelScope.launch {
            _state.value = when (section) {
                AdminSection.Accounts -> when (val r = client.accounts()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, accounts = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Aliases -> when (val r = client.aliases()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, aliases = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Domains -> when (val r = client.domains()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, domains = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Queue -> when (val r = client.queue()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, queue = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Dmarc -> when (val r = client.dmarcReports()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, dmarc = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Audit -> when (val r = client.auditLog()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, audit = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.AgentKeys -> when (val r = client.agentKeys()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, agentKeys = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Allowed -> when (val r = client.senderList(allowed = true)) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, allowedSenders = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Blocked -> when (val r = client.senderList(allowed = false)) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, blockedSenders = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Suppressed -> when (val r = client.suppressions()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, suppressed = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Groups -> when (val r = client.groups()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, groups = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.EmailGroups -> when (val r = client.emailGroups()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, emailGroups = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
                AdminSection.Apps -> when (val r = client.apps()) {
                    is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, apps = r.value)
                    is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
                }
            }
        }
    }

    fun closeAdmin() {
        _state.value = _state.value.copy(adminOpen = null, adminDetail = null, accountDetail = null)
    }

    /**
     * Open one group and read who is in it.
     *
     * Both kinds answer members under the same `members` key; only a
     * permission group has grants, and asking for them on an email
     * group would be a request that means nothing.
     */
    fun openAdminRow(section: AdminSection, row: jp.golia.mailrs.ui.AdminRow) {
        if (section == AdminSection.Accounts) {
            openAccount(row.key)
            return
        }
        val id = row.key.toLongOrNull() ?: return
        _state.value = _state.value.copy(
            adminDetail = AdminDetail(section, id, row.headline),
        )
        viewModelScope.launch {
            val members = when (section) {
                AdminSection.EmailGroups -> client.emailGroupMembers(id)
                AdminSection.Groups -> client.groupMembers(id)
                else -> return@launch
            }
            val grants = if (section == AdminSection.Groups) {
                client.groupPermissions(id)
            } else {
                MailrsClient.Outcome.Ok(emptyList())
            }
            val current = _state.value.adminDetail ?: return@launch
            if (current.id != id) return@launch
            _state.value = _state.value.copy(
                adminDetail = current.copy(
                    members = (members as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                    grants = (grants as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                    loading = false,
                ),
            )
        }
    }

    fun closeAdminRow() {
        _state.value = _state.value.copy(adminDetail = null)
    }

    private fun openAccount(address: String) {
        _state.value = _state.value.copy(accountDetail = AccountDetail(address))
        viewModelScope.launch {
            val quota = client.accountQuota(address)
            val sieve = client.accountSieve(address)
            val hooks = client.accountWebhooks(address)
            val current = _state.value.accountDetail ?: return@launch
            if (current.address != address) return@launch
            _state.value = _state.value.copy(
                accountDetail = current.copy(
                    quotaBytes = (quota as? MailrsClient.Outcome.Ok)?.value,
                    sieve = (sieve as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                    webhooks = (hooks as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                    loading = false,
                ),
            )
        }
    }

    fun closeAccount() {
        _state.value = _state.value.copy(accountDetail = null)
    }

    /**
     * Add or remove a member of an email group.
     *
     * Only email groups: a permission group's membership decides what
     * somebody may do, and granting that from a phone list — with no
     * confirmation and no record of why — is not an edit this offers.
     */
    fun addGroupMember(address: String) {
        val detail = _state.value.adminDetail ?: return
        if (detail.section != AdminSection.EmailGroups || address.isBlank()) return
        viewModelScope.launch {
            val r = client.addEmailGroupMember(detail.id, address.trim())
            if (r is MailrsClient.Outcome.Err) {
                _state.value = _state.value.copy(error = r.message)
                return@launch
            }
            reloadDetail(detail)
        }
    }

    fun removeGroupMember(address: String) {
        val detail = _state.value.adminDetail ?: return
        if (detail.section != AdminSection.EmailGroups) return
        viewModelScope.launch {
            val r = client.removeEmailGroupMember(detail.id, address)
            if (r is MailrsClient.Outcome.Err) {
                _state.value = _state.value.copy(error = r.message)
                return@launch
            }
            reloadDetail(detail)
        }
    }

    private suspend fun reloadDetail(detail: AdminDetail) {
        val members = client.emailGroupMembers(detail.id)
        val current = _state.value.adminDetail ?: return
        if (current.id != detail.id) return
        _state.value = _state.value.copy(
            adminDetail = current.copy(
                members = (members as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                loading = false,
            ),
        )
    }

    /**
     * Whether this list can be added to, and what the form asks for.
     *
     * Two fields at most, because an operator adding an alias on a
     * phone is doing it between other things. Anything that needs more
     * — an account, with a password — is not offered here at all rather
     * than offered badly.
     */
    fun addFields(section: AdminSection): List<String> = when (section) {
        AdminSection.Aliases -> listOf("Source address", "Target address")
        AdminSection.Domains -> listOf("Domain name")
        AdminSection.Allowed, AdminSection.Blocked -> listOf("Address")
        else -> emptyList()
    }

    /**
     * Create a row from what the form holds, then re-read the list.
     *
     * The alias's domain is taken from its own source address rather
     * than asked for separately: they cannot disagree, and a form that
     * lets them is a form that will be filled in wrong.
     */
    fun addAdminRow(section: AdminSection, values: List<String>) {
        viewModelScope.launch {
            val r = when (section) {
                AdminSection.Aliases -> {
                    val source = values.getOrElse(0) { "" }.trim()
                    val target = values.getOrElse(1) { "" }.trim()
                    if (source.isEmpty() || target.isEmpty()) return@launch
                    client.addAlias(
                        Admin.AddAliasRequest(
                            sourceAddress = source,
                            targetAddress = target,
                            domain = source.substringAfter('@', ""),
                        ),
                    )
                }
                AdminSection.Domains -> {
                    val name = values.getOrElse(0) { "" }.trim()
                    if (name.isEmpty()) return@launch
                    client.addDomain(name)
                }
                AdminSection.Allowed, AdminSection.Blocked -> {
                    val address = values.getOrElse(0) { "" }.trim()
                    if (address.isEmpty()) return@launch
                    client.addToSenderList(section == AdminSection.Allowed, address)
                }
                else -> return@launch
            }
            if (r is MailrsClient.Outcome.Err) {
                _state.value = _state.value.copy(error = r.message)
                return@launch
            }
            openAdmin(section)
        }
    }

    /**
     * Remove one row, and re-read the list.
     *
     * Re-read rather than removed locally: the server decides whether a
     * delete took, and a row that disappeared from the screen while the
     * request failed is the operator believing a thing is gone.
     */
    fun deleteAdminRow(section: AdminSection, row: jp.golia.mailrs.ui.AdminRow) {
        viewModelScope.launch {
            val r = when (section) {
                AdminSection.Aliases -> client.deleteAlias(row.key.toLongOrNull() ?: return@launch)
                AdminSection.Domains -> client.deleteDomain(row.key)
                AdminSection.AgentKeys ->
                    client.deleteAgentKey(row.key.toLongOrNull() ?: return@launch)
                AdminSection.Allowed -> client.removeFromSenderList(allowed = true, address = row.key)
                AdminSection.Blocked -> client.removeFromSenderList(allowed = false, address = row.key)
                AdminSection.Accounts, AdminSection.Queue, AdminSection.Dmarc,
                AdminSection.Audit, AdminSection.Suppressed,
                AdminSection.Groups, AdminSection.EmailGroups,
                AdminSection.Apps -> return@launch
            }
            if (r is MailrsClient.Outcome.Err) {
                _state.value = _state.value.copy(error = r.message)
                return@launch
            }
            openAdmin(section)
        }
    }

    /**
     * The message as it arrived, headers and all.
     *
     * What a mail server's operator reaches for when a message did not
     * do what it should have: the Received chain, the auth results, the
     * exact Content-Type. Nothing else in this app shows them.
     */
    fun viewSource(uid: Int) {
        _state.value = _state.value.copy(sourceOpen = true, source = null, error = null)
        viewModelScope.launch {
            _state.value = when (val r = client.messageSource(uid)) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(source = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(sourceOpen = false, error = r.message)
            }
        }
    }

    fun closeSource() {
        _state.value = _state.value.copy(sourceOpen = false, source = null)
    }

    /** Open and close the settings screen. */
    fun openSettings() {
        _state.value = _state.value.copy(settingsOpen = true, selected = emptySet())
    }

    fun closeSettings() {
        _state.value = _state.value.copy(settingsOpen = false)
    }

    /**
     * Choose light, dark, or the phone's own answer.
     *
     * Written through to the device store as it is chosen rather than on
     * some later save: a preference that is only in memory is one the
     * next launch forgets, and nobody sets a theme twice.
     */
    fun chooseAppearance(appearance: Prefs.Appearance) {
        prefs.appearance = appearance
        _state.value = _state.value.copy(appearance = appearance)
    }

    /**
     * The launcher's Search shortcut.
     *
     * State rather than a call into the screen: the shortcut can arrive
     * before the list is composed, and a flag the list reads when it
     * appears cannot be missed by arriving early.
     */
    fun openSearchFromShortcut() {
        _state.value = _state.value.copy(
            openSearch = true,
            open = null,
            composing = null,
            settingsOpen = false,
            draftsOpen = false,
            adminOpen = null,
        )
    }

    fun searchOpened() {
        if (!_state.value.openSearch) return
        _state.value = _state.value.copy(openSearch = false)
    }

    fun clearSearch() {
        searchToken++
        _state.value = _state.value.copy(searchTerm = "", results = null, searching = false)
    }

    /** Put the row back and forget the action. Nothing was ever sent. */
    fun undo() {
        val action = pending ?: return
        undoToken++
        pending = null
        _state.value = _state.value.copy(conversations = action.before, undo = null)
    }

    fun dismissUndo() {
        _state.value = _state.value.copy(undo = null)
    }

    private fun commit(action: PendingTriage) {
        pending = null
        _state.value = _state.value.copy(undo = null)
        viewModelScope.launch {
            when (val r = client.batch(action.verb, listOf(action.conversation.threadId))) {
                is MailrsClient.Outcome.Ok -> Unit
                is MailrsClient.Outcome.Err -> {
                    // It did not happen, so the row comes back. A row
                    // that vanished on a failed request is mail the
                    // person believes they filed and did not.
                    _state.value = _state.value.copy(
                        conversations = action.before,
                        error = r.message,
                    )
                }
            }
        }
    }

    fun closeThread() {
        _state.value = _state.value.copy(open = null, messages = emptyList())
    }

    fun dismissError() {
        _state.value = _state.value.copy(error = null)
    }

    data class UiState(
        val signedIn: Boolean = false,
        val server: String = "",
        val busy: Boolean = false,
        val error: String? = null,
        val conversations: List<Wire.Conversation> = emptyList(),
        /** The thread being read, or null for the list. */
        val open: Wire.Conversation? = null,
        val messages: List<Wire.Message> = emptyList(),
        /** The message being written, or null. */
        val composing: Draft? = null,
        val sending: Boolean = false,
        /** True for one frame after a send lands, so the list can say so. */
        val sent: Boolean = false,
        /** Whose mailbox this is — reply-all must not address it back. */
        val myAddress: String = "",
        /** What the snackbar is offering to undo, or null. */
        val undo: PendingTriage? = null,
        /** What was typed into the search field. */
        val searchTerm: String = "",
        /**
         * Hits, in the server's ranking. Null means no search is on —
         * distinct from an empty list, which means this term matched
         * nothing, and the two say different things to the reader.
         */
        val results: List<Wire.Conversation>? = null,
        val searching: Boolean = false,
        /** Set by the launcher shortcut; the list opens its search and clears it. */
        val openSearch: Boolean = false,
        /** Which list is showing. Its axes scope both the list and the search. */
        val list: MailList = MailList.Inbox,
        /** Saved drafts, newest first, and whether their list is showing. */
        val drafts: List<Wire.Draft> = emptyList(),
        val draftsOpen: Boolean = false,
        /** A draft was just saved; the list screen says so once. */
        val draftSaved: Boolean = false,
        /** The operator list showing, if any, and what it holds. */
        val adminOpen: AdminSection? = null,
        val accounts: List<Admin.Account> = emptyList(),
        val aliases: List<Admin.Alias> = emptyList(),
        val domains: List<Admin.Domain> = emptyList(),
        val queue: List<Admin.QueueJob> = emptyList(),
        val dmarc: List<Admin.DmarcReport> = emptyList(),
        val audit: List<Admin.AuditEntry> = emptyList(),
        val agentKeys: List<Admin.AgentKey> = emptyList(),
        val allowedSenders: List<String> = emptyList(),
        val blockedSenders: List<String> = emptyList(),
        val suppressed: List<String> = emptyList(),
        val groups: List<Admin.Group> = emptyList(),
        val emailGroups: List<Admin.EmailGroup> = emptyList(),
        val apps: List<Admin.App> = emptyList(),
        /** The account whose side state is showing, if one is. */
        val accountDetail: AccountDetail? = null,
        /** The group whose members are showing, if one is. */
        val adminDetail: AdminDetail? = null,
        /** The raw message being read, if any. Null while it is on its way. */
        val sourceOpen: Boolean = false,
        val source: String? = null,
        /** Whether the settings screen is showing. */
        val settingsOpen: Boolean = false,
        /** Light, dark, or the phone's own answer. */
        val appearance: Prefs.Appearance = Prefs.Appearance.System,
        /** Threads picked out for a bulk action. Empty means not selecting. */
        val selected: Set<String> = emptySet(),
        /** Contact suggestions for the field named by [suggestingFor]. */
        val suggestions: List<String> = emptyList(),
        val suggestingFor: RecipientField? = null,
        /** Which attachment index is being fetched, if any. */
        val openingAttachment: Int? = null,
        /** A file ready to hand to another app. */
        val openFile: OpenedFile? = null,
        /** Where each message's unsubscribe has got to, by uid. */
        val unsubscribing: Map<Int, Unsubscribing> = emptyMap(),
    )

    /**
     * An operator list, and how to read it.
     *
     * The rows come out of state rather than being fetched by the
     * screen, so the one place that knows what a row says is the same
     * place that knows what deleting one names.
     */
    enum class AdminSection(val title: String, val emptyMessage: String) {
        Accounts("Accounts", "No accounts on this server."),
        Aliases("Aliases", "Nothing forwards anywhere."),
        Domains("Domains", "This server answers for no domain."),
        Queue("Queue", "Nothing waiting to go out."),
        Dmarc("DMARC", "No reports yet."),
        Audit("Audit log", "Nothing has happened."),
        AgentKeys("Agent keys", "No keys act as this account."),
        Allowed("Always allowed", "Nothing skips the filter."),
        Blocked("Always blocked", "Nothing is refused on sight."),
        Suppressed("Suppressed", "The sender is retrying everybody."),
        Groups("Permission groups", "No groups are defined."),
        EmailGroups("Email groups", "No distribution addresses."),
        Apps("Apps", "Nothing holds credentials here.");

        fun rows(state: UiState): List<jp.golia.mailrs.ui.AdminRow> = when (this) {
            // Not deletable here: removing an account takes its mail
            // with it, and a delete button beside a list is not where
            // that decision belongs.
            Accounts -> state.accounts.map {
                jp.golia.mailrs.ui.AdminRow(
                    key = it.address,
                    headline = it.address,
                    detail = listOfNotNull(
                        it.displayName.takeIf(String::isNotBlank),
                        if (it.active) null else "inactive",
                        // A quota of zero is *no cap*, not a full
                        // mailbox, so it is left unsaid rather than
                        // printed as "0 B".
                        it.quotaBytes.takeIf { q -> q > 0 }
                            ?.let { q -> jp.golia.mailrs.ui.humanSize(q) },
                    ).joinToString(" · "),
                    deletable = false,
                    drillable = true,
                )
            }
            Aliases -> state.aliases.map {
                jp.golia.mailrs.ui.AdminRow(
                    key = it.id.toString(),
                    headline = it.sourceAddress + " → " + it.targetAddress,
                    detail = it.aliasType,
                    deletable = true,
                )
            }
            Domains -> state.domains.map {
                jp.golia.mailrs.ui.AdminRow(
                    key = it.name,
                    headline = it.name,
                    detail = "",
                    deletable = true,
                )
            }
            Queue -> state.queue.map { job ->
                // Asked for later is not stuck, and saying so is the
                // whole reason the row reads its own timestamps: a queue
                // where every row looks stuck is a queue nobody reads.
                val scheduled = job.scheduledAt
                val detail = when {
                    scheduled != null && scheduled > System.currentTimeMillis() / 1000 ->
                        "scheduled for " + jp.golia.mailrs.ui.RowDate.format(scheduled)
                    job.lastError != null ->
                        "attempt ${job.attempts ?: 0} — ${job.lastError}"
                    else -> job.status
                }
                jp.golia.mailrs.ui.AdminRow(
                    key = job.id.toString(),
                    headline = job.recipient.ifBlank { job.sender },
                    detail = detail,
                    deletable = false,
                )
            }
            Dmarc -> state.dmarc.map { r ->
                jp.golia.mailrs.ui.AdminRow(
                    key = r.sid,
                    headline = r.orgName.ifBlank { r.sid },
                    // Passing against total, because that is what a
                    // report is for. A count of rows says nothing about
                    // whether anybody's mail was refused.
                    detail = "${r.passing}/${r.total} passing · p=${r.p}",
                    deletable = false,
                )
            }
            AgentKeys -> state.agentKeys.map { k ->
                jp.golia.mailrs.ui.AdminRow(
                    key = k.id.toString(),
                    headline = k.name.ifBlank { k.prefix },
                    // The prefix and the scopes, because those are what
                    // tell two keys apart when one has to be revoked.
                    detail = (listOf(k.prefix) + k.scopes).joinToString(" · "),
                    deletable = true,
                )
            }
            Allowed -> state.allowedSenders.map {
                jp.golia.mailrs.ui.AdminRow(key = it, headline = it, detail = "", deletable = true)
            }
            Blocked -> state.blockedSenders.map {
                jp.golia.mailrs.ui.AdminRow(key = it, headline = it, detail = "", deletable = true)
            }
            // Not deletable one at a time: the endpoint clears the set,
            // and a delete button that quietly emptied the list would be
            // a different action wearing the same icon.
            Suppressed -> state.suppressed.map {
                jp.golia.mailrs.ui.AdminRow(key = it, headline = it, detail = "", deletable = false)
            }
            Groups -> state.groups.map { g ->
                jp.golia.mailrs.ui.AdminRow(
                    key = g.id.toString(),
                    headline = g.name,
                    // A builtin is cross-domain and cannot be edited
                    // away, so saying which is which is the first thing
                    // an operator needs from this list.
                    detail = listOfNotNull(
                        if (g.isBuiltin) "built in" else g.domain,
                        g.description.takeIf(String::isNotBlank),
                    ).joinToString(" · "),
                    deletable = false,
                    drillable = true,
                )
            }
            Apps -> state.apps.map { a ->
                jp.golia.mailrs.ui.AdminRow(
                    key = a.id.toString(),
                    headline = a.name.ifBlank { a.appId },
                    detail = listOfNotNull(
                        a.ownerAddress.takeIf(String::isNotBlank),
                        if (a.active) null else "inactive",
                        a.scopes.joinToString(", ").takeIf(String::isNotBlank),
                    ).joinToString(" · "),
                    deletable = false,
                )
            }
            EmailGroups -> state.emailGroups.map { g ->
                jp.golia.mailrs.ui.AdminRow(
                    key = g.id.toString(),
                    // The address, not the name: mail is sent to the
                    // address, and a list keyed on "Support" does not
                    // tell an operator what to type.
                    headline = g.address.ifBlank { g.name },
                    detail = listOfNotNull(
                        g.name.takeIf { it.isNotBlank() && it != g.address },
                        g.description.takeIf(String::isNotBlank),
                    ).joinToString(" · "),
                    deletable = false,
                    drillable = true,
                )
            }
            Audit -> state.audit.map { e ->
                jp.golia.mailrs.ui.AdminRow(
                    key = e.id.toString(),
                    headline = e.action + " " + e.target,
                    detail = listOf(e.actor, jp.golia.mailrs.ui.RowDate.format(e.timestamp))
                        .filter(String::isNotBlank)
                        .joinToString(" · "),
                    deletable = false,
                )
            }
        }
    }

    /** Which recipient line a suggestion belongs to. */
    enum class RecipientField { To, Cc, Bcc }

    /** A file picked for the message being written. */
    data class Attached(val uri: android.net.Uri, val filename: String, val size: Long)

    /** How far a one-click unsubscribe has got. */
    enum class Unsubscribing { Working, Done, Failed }

    /**
     * One account, opened.
     *
     * The three things about an account that are kept somewhere other
     * than the account row: how much it may hold, the sieve script that
     * files its mail, and what is subscribed to its events. All three
     * are read-only here — a sieve script is a program, and a phone
     * keyboard is the wrong place to edit one.
     */
    data class AccountDetail(
        val address: String,
        val quotaBytes: Long? = null,
        val sieve: String = "",
        val webhooks: List<Admin.Webhook> = emptyList(),
        val loading: Boolean = true,
    )

    /**
     * One group, opened.
     *
     * A group is a list with a list inside it, and the inner one is the
     * point: "Support" says nothing, "Support — lihao@golia.jp" is the
     * answer somebody came for. `grants` is only populated for a
     * permission group, where what it allows matters as much as who is
     * in it.
     */
    data class AdminDetail(
        val section: AdminSection,
        val id: Long,
        val title: String,
        val members: List<String> = emptyList(),
        val grants: List<String> = emptyList(),
        val loading: Boolean = true,
    )

    /** A downloaded attachment, waiting for the screen to hand it on. */
    data class OpenedFile(val file: java.io.File, val mimeType: String, val filename: String)

    /**
     * A triage waiting out its undo window.
     *
     * `before` is the whole list as it was, not just the row: putting
     * one row back at the right index is the same information and more
     * ways to get it wrong.
     */
    data class PendingTriage(
        val conversation: Wire.Conversation,
        val verb: MailrsClient.Verb,
        val before: List<Wire.Conversation>,
    )

    /**
     * A message being written.
     *
     * `id` exists so the composer's fields reset when a *different*
     * draft opens and not on every recomposition — `remember(draft.id)`
     * rather than `remember(Unit)`, which would keep the previous
     * reply's text when you opened a second one.
     */
    private companion object {
        /** Long enough to notice the row left, short enough not to feel stuck. */
        const val UNDO_WINDOW_MS = 5_000L
    }

    /**
     * The message being written, held here and nowhere else.
     *
     * The composer used to mirror these into its own `remember`d state
     * and hand them back on send. That is the pattern this codebase has
     * a rule against (`frontend/no-rq-mirror.md`), and it had a second
     * cost here: the back gesture cancels through the shell, which
     * cannot see a screen's local variables, so leaving by the gesture
     * everybody uses would have thrown the text away.
     *
     * `serverId` is null until the draft has been saved once; after
     * that it is reused, or one message leaves a trail of drafts.
     */
    data class Draft(
        val id: Int,
        val to: String = "",
        val cc: String = "",
        val bcc: String = "",
        val subject: String = "",
        val body: String = "",
        val inReplyTo: String? = null,
        val replyToThreadId: String? = null,
        val serverId: Long? = null,
        /**
         * Files picked to go with it.
         *
         * In memory only, and deliberately: a server draft has nowhere
         * to keep an attachment, and a `content://` URI granted to this
         * activity does not survive the process — a draft reopened
         * tomorrow with a file it can no longer read would be worse than
         * one that says it has none.
         */
        val attachments: List<Attached> = emptyList(),
    ) {
        /** Nothing typed and nothing quoted: not worth saving. */
        val isEmpty: Boolean
            get() = to.isBlank() && cc.isBlank() && bcc.isBlank() &&
                subject.isBlank() && body.isBlank()
    }
}
