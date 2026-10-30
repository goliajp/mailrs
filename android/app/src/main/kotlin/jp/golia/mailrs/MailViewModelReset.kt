package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.contactText
import jp.golia.mailrs.wire.resetPassword
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Open the reset-code form without a failed sign-in. */
fun MailViewModel.openReset(server: String, address: String) {
    _state.update { it.copy(reset = ResetPrompt(server, address), error = null, notice = null) }
}

fun MailViewModel.closeReset() {
    _state.update { it.copy(reset = null) }
}

/**
 * Set a new password with the code an issuer handed out. On success the
 * form closes and the sign-in screen says to sign in with it; nothing is
 * signed in here.
 */
fun MailViewModel.submitReset(address: String, code: String, password: String, repeated: String) {
    val prompt = state.value.reset ?: return
    if (password != repeated) {
        _state.update { it.copy(reset = prompt.copy(failure = "The two passwords are not the same.")) }
        return
    }
    _state.update { it.copy(reset = prompt.copy(busy = true, failure = null)) }
    viewModelScope.launch {
        when (val r = client.resetPassword(prompt.server, address.trim(), code, password)) {
            is MailrsClient.Outcome.Ok -> _state.update {
                it.copy(reset = null, notice = "Your new password is set. Sign in with it.", error = null)
            }
            is MailrsClient.Outcome.Err -> {
                val whom = prompt.contact ?: contactText("", "")
                val text = when (r.code) {
                    jp.golia.mailrs.wire.INVALID_RESET_CODE -> "${r.message} Ask $whom for a new code."
                    else -> r.message
                }
                _state.update { it.copy(reset = prompt.copy(busy = false, failure = text)) }
            }
        }
    }
}
