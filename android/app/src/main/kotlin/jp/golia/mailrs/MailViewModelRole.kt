package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.role
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Ask the server again what this account may do.
 *
 * Read on sign-in, whenever the app comes back to the front, and after any
 * 403, so a right taken away hides its pages without a new sign-in. A
 * failed read keeps what was known: an offline phone should not lose its
 * admin pages, and the server refuses them anyway if the right is gone.
 */
fun MailViewModel.refreshRole() {
    if (client.session == null) return
    viewModelScope.launch {
        val r = client.role()
        if (r is MailrsClient.Outcome.Ok) _state.update { it.copy(role = r.value) }
    }
}
