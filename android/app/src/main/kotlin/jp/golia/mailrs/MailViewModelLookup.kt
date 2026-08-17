package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailSignature
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.RecipientAutocomplete
import kotlinx.coroutines.launch

/**
 * Finding things, and the two small lookups that go with it.
 *
 * Extensions rather than methods, for the reason the other files give:
 * Kotlin has no partial classes and this view model keeps growing back
 * over the repo's 500-line limit. Search, its shortcut, the composer's
 * contact suggestions and the account's signature are all "ask the
 * server a small question", which is why they sit together.
 */
/**
 * Fetch the account's signature once, after signing in.
 *
 * Not on every compose: it changes about never, and a request per
 * message would be a request per message. A failure leaves it
 * empty, which signs nothing — the wrong signature would be worse
 * than none, and a person who cannot see why their mail is
 * unsigned can still type it.
 */
internal fun MailViewModel.loadSignature() {
    viewModelScope.launch {
        val r = client.signatures()
        if (r is MailrsClient.Outcome.Ok) {
            _state.value = _state.value.copy(signature = MailSignature.preferred(r.value))
        }
    }
}

/**
 * Search, with the server's ranking left alone.
 *
 * An empty term is not a search — it clears back to the inbox rather
 * than asking the server to rank everything.
 */
fun MailViewModel.search(term: String) {
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
 * The launcher's Search shortcut.
 *
 * State rather than a call into the screen: the shortcut can arrive
 * before the list is composed, and a flag the list reads when it
 * appears cannot be missed by arriving early.
 */
fun MailViewModel.openSearchFromShortcut() {
    _state.value = _state.value.copy(
        openSearch = true,
        open = null,
        composing = null,
        settingsOpen = false,
        draftsOpen = false,
        adminOpen = null,
    )
}

fun MailViewModel.searchOpened() {
    if (!_state.value.openSearch) return
    _state.value = _state.value.copy(openSearch = false)
}

fun MailViewModel.clearSearch() {
    searchToken++
    _state.value = _state.value.copy(searchTerm = "", results = null, searching = false)
}
