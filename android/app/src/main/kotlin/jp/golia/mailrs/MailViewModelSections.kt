package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.InboxSection
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.wire.acceptSenders
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Show one section of the Inbox, or the whole Inbox with null. */
fun MailViewModel.selectSection(section: InboxSection?) {
    val wire = section?.wire
    if (wire == _state.value.inboxSection) return
    _state.update { it.copy(inboxSection = wire, conversations = emptyList(), endOfList = false, selected = emptySet()) }
    refresh()
}

/** Accept the senders of the conversations shown in New, then reload: they leave the section. */
fun MailViewModel.acceptShownSenders() {
    val me = _state.value.role?.address.orEmpty()
    val senders = InboxSection.latestSenders(_state.value.conversations, me)
    if (senders.isEmpty()) return
    viewModelScope.launch {
        when (client.acceptSenders(senders)) {
            is MailrsClient.Outcome.Ok -> {
                _state.update { it.copy(conversations = emptyList(), endOfList = false) }
                refresh()
            }
            is MailrsClient.Outcome.Err ->
                _state.update { it.copy(error = Texts.get(R.string.section_accept_failed)) }
        }
    }
}
