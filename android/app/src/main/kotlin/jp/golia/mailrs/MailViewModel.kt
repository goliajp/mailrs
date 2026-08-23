package jp.golia.mailrs

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.SavedStateHandle
import kotlinx.serialization.json.Json
import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.widget.WidgetState
import jp.golia.mailrs.widget.refreshInboxWidgets
import jp.golia.mailrs.wire.Admin
import jp.golia.mailrs.wire.ContentUriBody
import jp.golia.mailrs.wire.MailCache
import jp.golia.mailrs.wire.MailList
import jp.golia.mailrs.wire.MailSignature
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
import kotlinx.coroutines.flow.update
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
class MailViewModel(
    app: Application,
    /**
     * Where the half-written message goes when the process does not
     * survive.
     *
     * A `ViewModel` outlives a rotation and nothing else. Android
     * reclaims a backgrounded app whenever it needs the memory — a
     * phone call, a camera, a large game — and brings it back looking
     * as it did, which for this app meant an empty composer where a
     * message had been. Leaving the composer saves a server draft, but
     * being killed is not leaving.
     *
     * Only the composer is kept. Everything else on screen is a copy of
     * what the server has and comes back with the next fetch; the draft
     * is the one thing that exists nowhere else yet.
     */
    private val saved: SavedStateHandle = SavedStateHandle(),
) : AndroidViewModel(app) {

    internal val client = MailrsClient(TokenStore(app))
    internal val prefs = Prefs(app)
    internal val cache = MailCache(app)
    internal var nextDraftId = 1
    internal var pending: PendingTriage? = null
    internal var undoToken = 0
    internal var searchToken = 0


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
            // Also here, not only after a fresh sign-in: a launch that
            // restores a session must know the address too, or reply-all
            // starts excluding nobody again the moment the app restarts.
            myAddress = client.session?.address.orEmpty(),
            appearance = prefs.appearance,
            notifyNewMail = prefs.notifyNewMail,
        )
    )
    val state: StateFlow<UiState> = _state.asStateFlow()


    init {
        // Restored before anything else touches the state, so a
        // recreated process opens on the message that was being
        // written rather than on the inbox behind it.
        saved.get<String>(DRAFT_KEY)?.let { stored ->
            runCatching { Json.decodeFromString(Draft.serializer(), stored) }
                .onSuccess { draft -> _state.update { it.copy(composing = draft) } }
        }

        // **Watched in one place rather than saved at ten.** Ten
        // functions set `composing`; a call to remember it in each is a
        // call somebody forgets to add to the eleventh. Collecting the
        // flow means every change is kept, including the ones written
        // by code that has never heard of saved state.
        //
        // The process is not warned before it is taken, so this happens
        // on every keystroke rather than on some later save.
        viewModelScope.launch {
            _state.collect { current ->
                saved[DRAFT_KEY] = current.composing?.let {
                    Json.encodeToString(Draft.serializer(), it)
                }
            }
        }

        // The server can stop accepting a session at any moment — a
        // token expires, an operator revokes it. Until this, the app
        // went on believing it was signed in and every request failed
        // with the same sentence.
        client.onSessionRejected = {
            // No message here on purpose. The request that hit the 401
            // writes its own — "Signed out — the server rejected this
            // session." — onto this reset state a moment later, and a
            // message set here would be overwritten by it. One owner
            // for the wording; this one owns the state.
            _state.value = UiState(
                appearance = prefs.appearance,
                notifyNewMail = prefs.notifyNewMail,
            )
            cache.clear()
            WidgetState.clear(getApplication())
        }

        if (client.session != null) {
            refresh()
            loadSignature()
        }
    }

    fun signIn(server: String, username: String, password: String) {
        _state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            when (val r = client.login(server, username, password)) {
                is MailrsClient.Outcome.Ok -> {
                    _state.update { it.copy(
                        signedIn = true,
                        busy = false,
                        server = client.session?.server.orEmpty(),
                        myAddress = client.session?.address.orEmpty(),
                    ) }
                    refresh()
                    loadSignature()
                }
                is MailrsClient.Outcome.Err ->
                    _state.update { it.copy(busy = false, error = r.message) }
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
        _state.update { it.copy(
            busy = true,
            error = null,
            // A refresh starts the paging over: the first page is the
            // newest fifty again, and whatever was paged in below it is
            // replaced rather than left dangling under fresh rows.
            endOfList = false,
            conversations = cached ?: _state.value.conversations,
        ) }
        viewModelScope.launch {
            when (val r = client.conversations((testFolder ?: list.axes).copy(accounts = _state.value.selectedAccounts))) {
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
                    _state.update { it.copy(busy = false, conversations = r.value) }
                }
                is MailrsClient.Outcome.Err ->
                    _state.update { it.copy(
                        busy = false,
                        error = if (_state.value.conversations.isEmpty()) r.message else null,
                    ) }
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
        _state.update { it.copy(loadingMore = true) }
        viewModelScope.launch {
            when (val r = client.conversations((testFolder ?: list.axes).copy(accounts = _state.value.selectedAccounts), before = before)) {
                is MailrsClient.Outcome.Ok -> {
                    // The list may have been switched while this was out.
                    if (_state.value.list != list) return@launch
                    val merged = ThreadPage.merge(_state.value.conversations, r.value)
                    _state.update { it.copy(
                        conversations = merged.rows,
                        loadingMore = false,
                        endOfList = !merged.progressed,
                    ) }
                }
                is MailrsClient.Outcome.Err ->
                    // Not the end — just not now. Saying otherwise would
                    // stop the list asking again after the network came
                    // back.
                    _state.update { it.copy(loadingMore = false) }
            }
        }
    }

    fun open(conversation: Wire.Conversation) {
        // Same rule as the list: the messages this thread had last time
        // are shown while the fetch is out, so opening mail already read
        // works with no network at all.
        val cached = cache.readMessages(conversation.threadId)
        _state.update { it.copy(
            open = conversation,
            messages = cached.orEmpty(),
            busy = true,
            error = null,
        ) }
        viewModelScope.launch {
            when (val r = client.thread(conversation.threadId)) {
                is MailrsClient.Outcome.Ok -> {
                    cache.writeMessages(r.value, conversation.threadId)
                    _state.update { it.copy(busy = false, messages = r.value) }
                    if (conversation.unreadCount > 0) {
                        client.markRead(conversation.threadId)
                        // Reflect it locally rather than refetching the
                        // whole list for one counter.
                        _state.update { it.copy(
                            conversations = _state.value.conversations.map {
                                if (it.threadId == conversation.threadId) it.copy(unreadCount = 0) else it
                            }
                        ) }
                    }
                }
                is MailrsClient.Outcome.Err ->
                    _state.update { it.copy(busy = false, error = r.message) }
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
        _state.update { it.copy(conversations = emptyList(), endOfList = false) }
        refresh()
    }

    fun forgetLoadedMail() {
        if (!BuildConfig.ALLOW_SERVER_OVERRIDE) return
        _state.update { it.copy(conversations = emptyList(), messages = emptyList()) }
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
     * Show another list.
     *
     * The rows on screen belong to the list that was showing, so they
     * are cleared rather than left under a new heading — a moment of
     * Junk labelled Inbox is worse than a moment of nothing.
     */
    fun show(list: MailList) {
        if (list == _state.value.list) return
        searchToken++
        _state.update { it.copy(
            list = list,
            selected = emptySet(),
            endOfList = false,
            conversations = emptyList(),
            searchTerm = "",
            results = null,
            searching = false,
            error = null,
        ) }
        refresh()
    }

















    /** Open and close the settings screen. */


    /**
     * Choose light, dark, or the phone's own answer.
     *
     * Written through to the device store as it is chosen rather than on
     * some later save: a preference that is only in memory is one the
     * next launch forgets, and nobody sets a theme twice.
     */





    /** Put the row back and forget the action. Nothing was ever sent. */







    internal companion object {
        const val DRAFT_KEY = "composing"
    }
}
