package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.CONFIRM_REQUIRED
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.confirm
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Run `action`; when the server answers that it wants the password typed
 * again, ask for it and run `action` once more.
 *
 * The changes a stolen session could use to stay in or to carry mail away
 * are refused with `confirm_required` until this session has confirmed its
 * password in the last ten minutes. Two refused at once share one prompt.
 */
internal suspend fun <T> MailViewModel.confirmed(
    action: suspend () -> MailrsClient.Outcome<T>,
): MailrsClient.Outcome<T> {
    val first = action()
    if (first !is MailrsClient.Outcome.Err || first.code != CONFIRM_REQUIRED) return first
    if (!askForPassword()) return first
    return action()
}

private suspend fun MailViewModel.askForPassword(): Boolean {
    confirmation?.let { return it.await() }
    val pending = CompletableDeferred<Boolean>()
    confirmation = pending
    _state.update { it.copy(confirm = ConfirmPrompt()) }
    try {
        return pending.await()
    } finally {
        confirmation = null
        _state.update { it.copy(confirm = null) }
    }
}

/** The prompt's Continue: send the password, and the code once asked for one. */
fun MailViewModel.submitConfirmation(password: String, code: String) {
    val pending = confirmation ?: return
    _state.update { it.copy(confirm = it.confirm?.copy(busy = true, message = null)) }
    viewModelScope.launch {
        val sentCode = code.trim().takeIf { _state.value.confirm?.needsCode == true && it.isNotEmpty() }
        when (val r = client.confirm(password, sentCode)) {
            is MailrsClient.Outcome.Ok -> pending.complete(true)
            is MailrsClient.Outcome.Err -> when (r.code) {
                "totp_required" -> _state.update {
                    it.copy(confirm = it.confirm?.copy(busy = false, needsCode = true, message = r.message))
                }
                "confirm_failed" -> _state.update {
                    it.copy(confirm = it.confirm?.copy(busy = false, message = r.message))
                }
                // signed out after too many tries, or the server could not be reached
                else -> {
                    _state.update { it.copy(error = r.message) }
                    pending.complete(false)
                }
            }
        }
    }
}

/** The prompt's Cancel: the change stays refused. */
fun MailViewModel.cancelConfirmation() {
    confirmation?.complete(false)
}
