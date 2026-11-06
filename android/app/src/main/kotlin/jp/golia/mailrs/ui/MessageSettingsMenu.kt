package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.UiState
import jp.golia.mailrs.askBlockSender
import jp.golia.mailrs.canBlock
import jp.golia.mailrs.fileSender
import jp.golia.mailrs.loadSenderLists
import jp.golia.mailrs.notifySender
import jp.golia.mailrs.notifyThread
import jp.golia.mailrs.unblockSender
import jp.golia.mailrs.wire.SenderIdentity
import jp.golia.mailrs.wire.SettingsWire
import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.wire.Wire
import kotlinx.coroutines.launch

/**
 * A message's own menu: where its sender's mail is always filed, whether
 * the sender notifies, whether this conversation does, and blocking the
 * sender. Settings the web keeps under Settings, reached here from the
 * message that prompted them. Not in the thread's menu, which is the
 * action table shared with the web.
 */
@Composable
fun MessageSettingsMenu(m: Wire.Message, threadId: String, vm: MailViewModel) {
    val address = SenderIdentity.emailOf(m.sender)
    var open by remember { mutableStateOf(false) }
    val state by vm.state.collectAsState()
    val scope = rememberCoroutineScope()
    // read again each time the menu opens: Settings may have changed them
    LaunchedEffect(open) { if (open) vm.loadSenderLists() }
    fun pick(action: suspend () -> Unit) {
        open = false
        scope.launch { action() }
    }
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.testTag("message.settings.${m.uid}")) {
            Icon(Icons.Filled.MoreVert, Texts.get(R.string.message_settings_menu), tint = LocalTheme.current.fgSecondary)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            if (address.isNotEmpty()) {
                for (bucket in SettingsWire.Bucket.entries) {
                    Item(Texts.get(R.string.sender_file_to, Texts.get(bucketLabel(bucket))), "sender.file.${bucket.wire}") {
                        pick { vm.fileSender(address, bucket) }
                    }
                }
                HorizontalDivider()
                Item(Texts.get(R.string.sender_notify_always), "sender.notify.always") { pick { vm.notifySender(address, "always") } }
                Item(Texts.get(R.string.sender_notify_never), "sender.notify.never") { pick { vm.notifySender(address, "never") } }
                Item(Texts.get(R.string.sender_notify_default), "sender.notify.default") { pick { vm.notifySender(address, null) } }
                HorizontalDivider()
                BlockItem(address, state) { blocked ->
                    open = false
                    when (blocked) {
                        true -> vm.unblockSender(address)
                        false -> vm.askBlockSender(address)
                    }
                }
            }
            Item(Texts.get(R.string.thread_notify_follow), "thread.notify.follow") { pick { vm.notifyThread(threadId, "follow") } }
            Item(Texts.get(R.string.thread_notify_mute), "thread.notify.mute") { pick { vm.notifyThread(threadId, "mute") } }
            Item(Texts.get(R.string.thread_notify_default), "thread.notify.default") { pick { vm.notifyThread(threadId, null) } }
        }
    }
}

/**
 * "Block x@y", or "Unblock x@y" when it is on the blacklist. Nothing until
 * the lists are read, when neither word is known to be right, and nothing
 * for the reader's own address. [onPick] is told whether it was blocked.
 */
@Composable
private fun BlockItem(address: String, state: UiState, onPick: (Boolean) -> Unit) {
    val lists = state.senderLists ?: return
    if (!state.canBlock(address)) return
    val addr = address.lowercase()
    when (addr in lists.blocked) {
        true -> Item(Texts.get(R.string.block_unblock_item, addr), "sender.unblock") { onPick(true) }
        false -> Item(Texts.get(R.string.block_item, addr), "sender.block") { onPick(false) }
    }
}

/**
 * The question before a block: whom, what happens to their mail, where it
 * is undone, and that it comes off the allowed list if it is there.
 */
@Composable
fun BlockSenderDialog(address: String, allowed: Boolean, onAnswer: (Boolean) -> Unit) {
    val theme = LocalTheme.current
    AlertDialog(
        onDismissRequest = { onAnswer(false) },
        containerColor = theme.surface,
        title = { Text(stringResource(R.string.block_confirm_title, address), color = theme.fg) },
        text = {
            Column {
                Text(stringResource(R.string.block_confirm_body), color = theme.fgSecondary)
                if (allowed) {
                    Text(
                        stringResource(R.string.block_confirm_allowed, address),
                        color = theme.fgSecondary,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { onAnswer(true) }, modifier = Modifier.testTag("button.blockSender")) {
                Text(stringResource(R.string.block_confirm_action), color = theme.danger)
            }
        },
        dismissButton = {
            TextButton(onClick = { onAnswer(false) }) { Text(stringResource(R.string.common_cancel)) }
        },
    )
}

@Composable
private fun Item(text: String, tag: String, onClick: () -> Unit) {
    DropdownMenuItem(text = { Text(text) }, onClick = onClick, modifier = Modifier.testTag(tag))
}

private fun bucketLabel(bucket: SettingsWire.Bucket): Int = when (bucket) {
    SettingsWire.Bucket.Inbox -> R.string.bucket_inbox
    SettingsWire.Bucket.Notifications -> R.string.bucket_notifications
    SettingsWire.Bucket.Promotions -> R.string.bucket_promotions
}
