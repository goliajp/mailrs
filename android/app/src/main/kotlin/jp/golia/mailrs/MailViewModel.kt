package jp.golia.mailrs

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.widget.WidgetState
import jp.golia.mailrs.widget.refreshInboxWidgets
import jp.golia.mailrs.wire.Admin
import jp.golia.mailrs.wire.ContentUriBody
import jp.golia.mailrs.wire.MailCache
import jp.golia.mailrs.wire.MailList
import jp.golia.mailrs.wire.MailListAxes
import jp.golia.mailrs.wire.messageSource
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.NewMailWorker
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.RecentRecipients
import jp.golia.mailrs.wire.RecipientAutocomplete
import jp.golia.mailrs.wire.ShareIntent
import jp.golia.mailrs.wire.ReplyRecipients
import jp.golia.mailrs.wire.TokenStore
import jp.golia.mailrs.wire.ThreadPage
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

    internal val client = MailrsClient(TokenStore(app))
    internal val prefs = Prefs(app)
    internal val cache = MailCache(app)
    internal var nextDraftId = 1
    internal var pending: PendingTriage? = null
    internal var undoToken = 0
    private var searchToken = 0
    internal var contactToken = 0

    /** Set only by `useFolderForTest`; null in every shipped build. */
    internal var testFolder: MailListAxes? = null

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
        // **Null means "nothing was given", not "forget what you have".**
        // The activity calls this on every composition, and a
        // configuration change — a rotation, or a display resize — makes
        // a new activity whose intent carries no override. Clearing on
        // null wiped it, and the next request went to the real host and
        // came back "Failure in SSL library".
        if (url == null) return
        client.baseUrlOverride = url
    }

    internal val _state = MutableStateFlow(
        UiState(
            signedIn = client.session != null,
            server = client.session?.server.orEmpty(),
            appearance = prefs.appearance,
            notifyNewMail = prefs.notifyNewMail,
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
        // The mailbox goes with the session. A cached list left behind
        // would paint somebody else's mail onto the next sign-in for as
        // long as the first fetch takes.
        cache.clear()
        // And the share sheet's top row, which is a list of people this
        // account writes to and belongs to the account.
        RecentRecipients.clear(getApplication())
        // And the widget, or the launcher would keep showing this
        // account's mail to whoever signs in next.
        WidgetState.clear(getApplication())
        viewModelScope.launch { refreshInboxWidgets(getApplication()) }
        _state.value = UiState(appearance = prefs.appearance, notifyNewMail = prefs.notifyNewMail)
        // Nothing to check for once nobody is signed in, and a count
        // left behind would make the next person's first check
        // announce a difference against somebody else's mailbox.
        prefs.lastUnseen = null
        NewMailWorker.schedule(getApplication(), false)
    }

    /**
     * Fetch the list, showing what was last seen while it is out.
     *
     * **The cache paints first.** A cold launch on a train used to be a
     * spinner and then "Could not reach the server", for a mailbox the
     * phone had fetched two minutes earlier. Rows appear at once and are
     * replaced when the answer arrives.
     *
     * A failure with rows on screen keeps them and says nothing: they
     * are still the last true thing anybody knew, and an error banner
     * over readable mail is noise. A failure with *no* rows is the
     * screen that has to explain itself.
     */
    fun refresh() {
        val list = _state.value.list
        val cached = if (_state.value.conversations.isEmpty()) cache.readConversations(list.name) else null
        _state.value = _state.value.copy(
            busy = true,
            error = null,
            // A refresh starts the paging over: the first page is the
            // newest fifty again, and whatever was paged in below it is
            // replaced rather than left dangling under fresh rows.
            endOfList = false,
            conversations = cached ?: _state.value.conversations,
        )
        viewModelScope.launch {
            when (val r = client.conversations(testFolder ?: list.axes)) {
                is MailrsClient.Outcome.Ok -> {
                    cache.writeConversations(r.value, list.name)
                    // The home-screen widget draws what was last
                    // fetched and never fetches itself — it is redrawn
                    // on every launcher scroll. Written here because
                    // this is where the list is already in hand.
                    WidgetState.write(getApplication(), signedIn = true, conversations = r.value)
                    viewModelScope.launch { refreshInboxWidgets(getApplication()) }
                    // Still the list that was asked for: switching lists
                    // mid-flight must not paint the old one's answer.
                    if (_state.value.list != list) return@launch
                    _state.value = _state.value.copy(busy = false, conversations = r.value)
                }
                is MailrsClient.Outcome.Err ->
                    _state.value = _state.value.copy(
                        busy = false,
                        error = if (_state.value.conversations.isEmpty()) r.message else null,
                    )
            }
        }
    }

    /**
     * Fetch the next page when the list has been scrolled to its end.
     *
     * The mailbox is thousands of threads and a page is fifty, so a list
     * that stopped at the first page put everything older than the last
     * fortnight out of reach. Keyset paging, with the boundary second
     * re-requested and merged — [ThreadPage] carries the reasoning.
     *
     * A page with nothing new in it is the end, and `endOfList` stops
     * the asking; a full page of rows already held would otherwise be
     * requested forever.
     */
    fun loadMore() {
        val state = _state.value
        if (state.loadingMore || state.endOfList || state.conversations.isEmpty()) return
        val before = ThreadPage.nextBefore(state.conversations) ?: return
        val list = state.list
        _state.value = state.copy(loadingMore = true)
        viewModelScope.launch {
            when (val r = client.conversations(testFolder ?: list.axes, before = before)) {
                is MailrsClient.Outcome.Ok -> {
                    // The list may have been switched while this was out.
                    if (_state.value.list != list) return@launch
                    val merged = ThreadPage.merge(_state.value.conversations, r.value)
                    _state.value = _state.value.copy(
                        conversations = merged.rows,
                        loadingMore = false,
                        endOfList = !merged.progressed,
                    )
                }
                is MailrsClient.Outcome.Err ->
                    // Not the end — just not now. Saying otherwise would
                    // stop the list asking again after the network came
                    // back.
                    _state.value = _state.value.copy(loadingMore = false)
            }
        }
    }

    fun open(conversation: Wire.Conversation) {
        // Same rule as the list: the messages this thread had last time
        // are shown while the fetch is out, so opening mail already read
        // works with no network at all.
        val cached = cache.readMessages(conversation.threadId)
        _state.value = _state.value.copy(
            open = conversation,
            messages = cached.orEmpty(),
            busy = true,
            error = null,
        )
        viewModelScope.launch {
            when (val r = client.thread(conversation.threadId)) {
                is MailrsClient.Outcome.Ok -> {
                    cache.writeMessages(r.value, conversation.threadId)
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
     * Forget the mail held in memory, keeping the session and the disk
     * cache — a cold launch, without the launch.
     *
     * Debug only, and it exists because the alternative does not work:
     * a `ViewModel` survives activity recreation by design, so there is
     * no way from a test to get a fresh one and watch it paint from
     * disk. Without this the cache path has no coverage at all, and an
     * untested cache is one that silently stops being read.
     */
    /**
     * Point the list at a folder this enum does not name.
     *
     * Debug only, and it exists for the stub's `Paged` fixture: 120
     * threads with a deliberate collision at rows 48-52, which is the
     * only way to page against a server-shaped answer rather than a
     * two-row fixture.
     */
    fun useFolderForTest(folder: String) {
        if (!BuildConfig.ALLOW_SERVER_OVERRIDE) return
        testFolder = MailList.named(folder)
        _state.value = _state.value.copy(conversations = emptyList(), endOfList = false)
        refresh()
    }

    fun forgetLoadedMail() {
        if (!BuildConfig.ALLOW_SERVER_OVERRIDE) return
        _state.value = _state.value.copy(conversations = emptyList(), messages = emptyList())
    }

    /** Every keystroke, straight into the one copy of the draft. */



    /** Load the drafts list, and open one for editing. */




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






    /** The screen has handed it to another app; stop offering it. */


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
            endOfList = false,
            conversations = emptyList(),
            searchTerm = "",
            results = null,
            searching = false,
            error = null,
        )
        refresh()
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
    /**
     * Turn the periodic new-mail check on or off.
     *
     * Scheduling follows immediately: a switch that only takes effect
     * next launch is a switch that looks broken.
     */
    fun chooseNotify(on: Boolean) {
        prefs.notifyNewMail = on
        NewMailWorker.schedule(getApplication(), on)
        _state.value = _state.value.copy(notifyNewMail = on)
    }

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



    /**
     * Open a thread named by something outside the app — a tapped
     * notification.
     *
     * The list is fetched first because a thread needs its row for the
     * header, and the row is what the notification did not carry. If it
     * is not there any more — read elsewhere, archived elsewhere — the
     * inbox is what opens, which is the honest answer rather than an
     * empty thread.
     */
    fun openThreadById(threadId: String) {
        viewModelScope.launch {
            val rows = client.conversations(_state.value.list)
            if (rows is MailrsClient.Outcome.Ok) {
                _state.value = _state.value.copy(conversations = rows.value)
                rows.value.firstOrNull { it.threadId == threadId }?.let { open(it) }
            }
        }
    }

    fun closeThread() {
        _state.value = _state.value.copy(open = null, messages = emptyList())
    }

    fun dismissError() {
        _state.value = _state.value.copy(error = null)
    }

}
