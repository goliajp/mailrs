package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.NewMailWorker
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.messageSource
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Opening and closing the screens that are not mail: settings, the
 * message source, and the small things that go with them.
 *
 * Extensions, for the reason the rest of this view model gives — Kotlin
 * has no partial classes, and every feature added pushes the file back
 * over the repo's 500-line limit.
 */
/**
 * The message as it arrived, headers and all.
 *
 * What a mail server's operator reaches for when a message did not
 * do what it should have: the Received chain, the auth results, the
 * exact Content-Type. Nothing else in this app shows them.
 */
fun MailViewModel.viewSource(uid: Int) {
    _state.update { it.copy(sourceOpen = true, source = null, error = null) }
    viewModelScope.launch {
        _state.update {
            when (val r = client.messageSource(uid)) {
            is MailrsClient.Outcome.Ok -> it.copy(source = r.value)
            is MailrsClient.Outcome.Err -> it.copy(sourceOpen = false, error = r.message)
        }
        }
    }
}

fun MailViewModel.closeSource() {
    _state.update { it.copy(sourceOpen = false, source = null) }
}

fun MailViewModel.openSettings() {
    _state.update { it.copy(settingsOpen = true, selected = emptySet()) }
}

fun MailViewModel.closeSettings() {
    _state.update { it.copy(settingsOpen = false) }
}

/**
 * Turn the periodic new-mail check on or off.
 *
 * Scheduling follows immediately: a switch that only takes effect
 * next launch is a switch that looks broken.
 */
fun MailViewModel.chooseNotify(on: Boolean) {
    prefs.notifyNewMail = on
    NewMailWorker.schedule(getApplication(), on)
    _state.update { it.copy(notifyNewMail = on) }
}

fun MailViewModel.chooseAppearance(appearance: Prefs.Appearance) {
    prefs.appearance = appearance
    _state.update { it.copy(appearance = appearance) }
}

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
fun MailViewModel.openThreadById(threadId: String) {
    viewModelScope.launch {
        val rows = client.conversations(_state.value.list)
        if (rows is MailrsClient.Outcome.Ok) {
            _state.update { it.copy(conversations = rows.value) }
            rows.value.firstOrNull { it.threadId == threadId }?.let { open(it) }
        }
    }
}

fun MailViewModel.closeThread() {
    _state.update { it.copy(open = null, messages = emptyList()) }
}

/**
 * Something outside this view model went wrong and is worth saying.
 *
 * A screen can reach the parts of Android that fail — no app to
 * open a file, no browser for a link — and those failures belong on
 * the same snackbar as everything else rather than in a `runCatching`
 * nobody reads.
 */
fun MailViewModel.reportFailure(message: String) {
    _state.update { it.copy(error = message) }
}

fun MailViewModel.dismissError() {
    _state.update { it.copy(error = null) }
}
