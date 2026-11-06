package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.InboxSection
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.SectionedInboxSetting
import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.wire.acceptSenders
import jp.golia.mailrs.wire.saveSectionedInboxSetting
import jp.golia.mailrs.wire.sectionedInboxSetting
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Show one section of the Inbox, or the whole Inbox with null. */
fun MailViewModel.selectSection(section: InboxSection?) {
    val wire = section?.wire
    if (wire == _state.value.inboxSection) return
    _state.update { it.copy(inboxSection = wire, conversations = emptyList(), endOfList = false, selected = emptySet()) }
    refresh()
}

/** The sectioned inbox setting from the server. A failure keeps what is there and says nothing: the Inbox works either way. */
fun MailViewModel.loadSectionedInbox() {
    viewModelScope.launch {
        when (val r = client.sectionedInboxSetting()) {
            is MailrsClient.Outcome.Ok -> showSections(r.value.enabled)
            is MailrsClient.Outcome.Err -> Unit
        }
    }
}

/** Turn the section tabs on or off; they change at once and go back if the server does not take it. */
fun MailViewModel.saveSectionedInbox(enabled: Boolean) {
    val before = _state.value.sectionedInbox
    showSections(enabled)
    viewModelScope.launch {
        when (client.saveSectionedInboxSetting(SectionedInboxSetting(enabled))) {
            is MailrsClient.Outcome.Ok -> Unit
            is MailrsClient.Outcome.Err -> {
                _state.update { it.copy(error = Texts.get(R.string.sections_save_failed)) }
                showSections(before)
            }
        }
    }
}

/** With the tabs gone a section can no longer be left, so the whole Inbox comes back in its place. */
private fun MailViewModel.showSections(enabled: Boolean) {
    _state.update { it.copy(sectionedInbox = enabled) }
    if (!enabled) selectSection(null)
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
