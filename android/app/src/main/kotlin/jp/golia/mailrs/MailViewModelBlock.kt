package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.MailList
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.RowAction
import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.wire.blockSender
import jp.golia.mailrs.wire.removeFromSenderList
import jp.golia.mailrs.wire.senderList
import jp.golia.mailrs.wire.unblockSender
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * What the reader has on their two sender lists, lowercased. An address
 * is on one at most: the server takes it off one when it goes on the
 * other, and this copy does the same.
 */
data class SenderLists(val blocked: Set<String> = emptySet(), val allowed: Set<String> = emptySet()) {
    fun block(address: String) = SenderLists(blocked + address, allowed - address)
    fun allow(address: String) = SenderLists(blocked - address, allowed + address)
    fun unblock(address: String) = copy(blocked = blocked - address)
}

/** A block the undo snackbar can take back. */
data class BlockedSender(
    val threadId: String,
    val address: String,
    /** It came off the whitelist, and goes back on with the undo. */
    val wasAllowed: Boolean,
)

/** Whether a message menu offers Block for [address]: not the reader's own, whose replies would go to Junk too. */
fun UiState.canBlock(address: String): Boolean {
    val addr = address.lowercase()
    return addr.contains('@') && addr != myAddress.lowercase()
}

/** Read both lists, for the menu to know whether to offer Block or Unblock. */
fun MailViewModel.loadSenderLists() {
    viewModelScope.launch {
        val blocked = client.senderList(allowed = false)
        val allowed = client.senderList(allowed = true)
        if (blocked is MailrsClient.Outcome.Ok && allowed is MailrsClient.Outcome.Ok) {
            val lists = SenderLists(
                blocked.value.map { it.lowercase() }.toSet(),
                allowed.value.map { it.lowercase() }.toSet(),
            )
            _state.update { it.copy(senderLists = lists) }
        }
    }
}

/** Ask before blocking: the dialog names the address and where its mail will go. */
fun MailViewModel.askBlockSender(address: String) {
    _state.update { it.copy(confirmBlock = address.trim().lowercase()) }
}

/**
 * The answer to [askBlockSender]. Yes blacklists the sender and moves the
 * open conversation to Junk in one call; the thread closes (unless the
 * list is Junk) and the snackbar offers the undo.
 */
fun MailViewModel.answerBlockSender(yes: Boolean) {
    val address = _state.value.confirmBlock
    _state.update { it.copy(confirmBlock = null) }
    val open = _state.value.open
    if (!yes || address == null || open == null) return
    val inJunk = _state.value.list == MailList.Junk
    if (!inJunk) closeThread()
    viewModelScope.launch {
        when (val r = client.blockSender(open.threadId, address)) {
            is MailrsClient.Outcome.Ok -> {
                val before = _state.value.conversations
                val after = when (inJunk) {
                    true -> before
                    false -> before.filterNot { it.threadId == open.threadId }
                }
                val blocked = BlockedSender(open.threadId, r.value.address, r.value.wasAllowed)
                val undo = PendingTriage(listOf(open), RowAction.MarkJunk, before, applied = true, blocked = blocked)
                _state.update {
                    it.copy(conversations = after, undo = undo, senderLists = it.senderLists?.block(r.value.address))
                }
            }
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}

/** Off the blacklist; the conversation stays where it is. */
fun MailViewModel.unblockSender(address: String) {
    val addr = address.trim().lowercase()
    viewModelScope.launch {
        when (val r = client.removeFromSenderList(allowed = false, address = addr)) {
            is MailrsClient.Outcome.Ok -> _state.update {
                it.copy(senderLists = it.senderLists?.unblock(addr), error = Texts.get(R.string.block_unblocked, addr))
            }
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}

/**
 * The undo of a block: off the blacklist, back on the whitelist if it came
 * from there, and the conversation back to the Inbox. The rows are already
 * back on screen.
 */
internal fun MailViewModel.undoBlock(blocked: BlockedSender) {
    viewModelScope.launch {
        when (val r = client.unblockSender(blocked.threadId, blocked.address, blocked.wasAllowed)) {
            is MailrsClient.Outcome.Ok -> _state.update {
                val lists = when (blocked.wasAllowed) {
                    true -> it.senderLists?.allow(blocked.address)
                    false -> it.senderLists?.unblock(blocked.address)
                }
                it.copy(senderLists = lists)
            }
            is MailrsClient.Outcome.Err -> {
                _state.update { it.copy(error = r.message) }
                refresh()
            }
        }
    }
}
