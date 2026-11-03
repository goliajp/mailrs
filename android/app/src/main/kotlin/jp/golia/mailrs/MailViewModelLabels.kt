package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailList
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.WireTriage
import jp.golia.mailrs.wire.labels
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * The reader's labels: listed in the drawer, each one a list, and put on
 * or taken off a conversation.
 */

/**
 * What the cache files a list under. A label is its own list, so its
 * rows are not shown under another label's heading on the next start.
 */
fun UiState.listKey(): String = when (val l = label) {
    null -> list.name
    else -> "Label-" + l.id
}

/** Fetch the labels, for the drawer and the picker. A failure leaves the last ones known. */
fun MailViewModel.loadLabels() {
    viewModelScope.launch {
        when (val r = client.labels()) {
            is MailrsClient.Outcome.Ok -> _state.update { it.copy(labels = r.value) }
            is MailrsClient.Outcome.Err -> Unit
        }
    }
}

/** Show one label's conversations. */
fun MailViewModel.showLabel(label: WireTriage.Label) {
    show(MailList.Label, label)
}
