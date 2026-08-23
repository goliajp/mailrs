package jp.golia.mailrs.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.accounts.AccountStore
import jp.golia.mailrs.accounts.MailAccount
import jp.golia.mailrs.accounts.MailboxRow
import jp.golia.mailrs.accounts.MessageAttachments
import jp.golia.mailrs.accounts.SafeFilename
import jp.golia.mailrs.accounts.MailAddresses
import jp.golia.mailrs.accounts.MessageReader
import jp.golia.mailrs.accounts.ReplyDraft
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** One message from a connected mailbox, open. */
@Composable
fun MessageScreen(
    row: MailboxRow,
    account: MailAccount?,
    onReply: (MessageReader.Loaded, Boolean) -> Unit,
    onClose: () -> Unit,
) {
    val theme = LocalTheme.current
    val context = LocalContext.current
    val store = remember { AccountStore(context) }
    var outcome by remember(row.id) { mutableStateOf<MessageReader.Outcome?>(null) }
    var opening by remember { mutableStateOf<MessageAttachments.Attachment?>(null) }
    var openFailure by remember { mutableStateOf("") }

    // Handing the file on is an action, so it happens once per file and
    // then the offer is cleared. Leaving it in state would re-open the
    // same attachment on every recomposition.
    LaunchedEffect(opening) {
        val attachment = opening ?: return@LaunchedEffect
        opening = null
        openFailure = openOnThisPhone(context, row.id, attachment)
    }

    BackHandler { onClose() }

    LaunchedEffect(row.id) {
        outcome = when (account) {
            null -> MessageReader.Outcome.Failed("This mailbox is no longer connected")
            else -> MessageReader.load(account, row, store)
        }
    }

    Column(
        Modifier.fillMaxSize().background(theme.bg).verticalScroll(rememberScrollState()),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(onClick = onClose, modifier = Modifier.testTag("message.back")) {
                Text("Back", color = theme.accent, fontSize = 13.sp)
            }
            androidx.compose.foundation.layout.Box(Modifier.weight(1f))
            // Offered only once there is a message to reply to: the
            // recipient, the subject and the threading all come out of
            // headers that have not arrived yet.
            (outcome as? MessageReader.Outcome.Ok)?.let { ok ->
                TextButton(
                    onClick = { onReply(ok.loaded, false) },
                    modifier = Modifier.testTag("message.reply"),
                ) {
                    Text("Reply", color = theme.accent, fontSize = 13.sp)
                }
                // Offered only when there is somebody else on it.
                // "Reply all" over a message with one recipient does
                // the same thing as Reply and invites the mistake it
                // is named for.
                if (hasOthers(ok.loaded, account)) {
                    TextButton(
                        onClick = { onReply(ok.loaded, true) },
                        modifier = Modifier.testTag("message.replyAll"),
                    ) {
                        Text("Reply all", color = theme.accent, fontSize = 13.sp)
                    }
                }
            }
        }
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                row.displaySubject,
                color = theme.fg,
                fontSize = 16.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.testTag("message.subject"),
            )
            Text(row.displaySender, color = theme.fgSecondary, fontSize = 13.sp)
            Text(whenAndWhere(row, account), color = theme.fgMuted, fontSize = 11.sp)
        }
        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            when (val o = outcome) {
                null -> Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    Text("Fetching…", color = theme.fgMuted, fontSize = 13.sp)
                }
                is MessageReader.Outcome.Failed -> Text(
                    o.why,
                    color = theme.fgMuted,
                    fontSize = 13.sp,
                    modifier = Modifier.testTag("message.failed"),
                )
                is MessageReader.Outcome.Ok -> {
                    if (o.loaded.text.isEmpty()) {
                        Text(
                            "This message has no text to show.",
                            color = theme.fgMuted,
                            fontSize = 13.sp,
                        )
                    } else {
                        // Selectable, because half of what people do with
                        // a message is copy a code or an address out of it.
                        SelectionContainer {
                            Text(
                                o.loaded.text,
                                color = theme.fg,
                                fontSize = 14.sp,
                                modifier = Modifier.testTag("message.body"),
                            )
                        }
                    }
                    if (o.loaded.attachments.isNotEmpty()) {
                        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                        for (attachment in o.loaded.attachments) {
                            AttachmentRow(attachment) { opening = attachment }
                        }
                    }
                    if (openFailure.isNotEmpty()) {
                        Text(openFailure, color = theme.fgMuted, fontSize = 11.sp)
                    }
                    if (o.loaded.fromHtml) {
                        // Said plainly rather than hidden: a formatted
                        // message shown as text reads as broken unless
                        // something says why, and the why is that no
                        // remote image gets to report that this was read.
                        Text(
                            "Shown as text. Images and formatting are not loaded.",
                            color = theme.fgMuted,
                            fontSize = 11.sp,
                        )
                    }
                }
            }
        }
    }
}

private fun whenAndWhere(row: MailboxRow, account: MailAccount?): String {
    val parts = mutableListOf<String>()
    row.date?.let {
        parts.add(
            SimpleDateFormat("d MMM yyyy HH:mm", Locale.getDefault()).format(Date(it * 1000)),
        )
    }
    parts.add("${account?.title ?: "Unknown"} · ${row.folder}")
    return parts.joinToString(" · ")
}

/** Whether anybody besides the sender and this account was on it. */
private fun hasOthers(loaded: MessageReader.Loaded, account: MailAccount?): Boolean {
    val mine = account?.address ?: return false
    return MailAddresses.replyAll(
        loaded.headers.to,
        loaded.headers.cc,
        ReplyDraft.recipient(loaded.headers),
        mine,
    ).isNotEmpty()
}

/**
 * One attached file.
 *
 * Named, sized and typed — the three things somebody needs to decide
 * whether to open it, and the size especially: a 40 MB file on a phone
 * on mobile data is a decision, not a tap.
 */
@Composable
private fun AttachmentRow(
    attachment: MessageAttachments.Attachment,
    onOpen: () -> Unit,
) {
    val theme = LocalTheme.current
    Column(
        Modifier
            .clickable { onOpen() }
            .padding(vertical = 4.dp)
            .testTag("message.attachment"),
    ) {
        Text(attachment.filename, color = theme.fg, fontSize = 13.sp)
        Text(
            // The existing one, in `Attachments.kt`. A second size
            // formatter is a second answer to the same question, and
            // adding an `Int` overload beside its `Long` one silently
            // stole every call made with a literal — which is how a
            // test of the old one went red on a change that never
            // touched it.
            "${attachment.mimeType} · ${humanSize(attachment.size.toLong())}",
            color = theme.fgMuted,
            fontSize = 11.sp,
        )
    }
}

/**
 * Put the bytes somewhere another app can read, and hand them over.
 *
 * A per-message directory, because **the filename is the sender's and
 * is not unique**: two messages carrying `invoice.pdf` would otherwise
 * overwrite each other's. A `FileProvider` URI is what leaves this app
 * — handing out a `file://` path has been an error since API 24, and a
 * world-readable copy would be a worse way to make it work.
 *
 * @return why it could not be opened, or empty when it was.
 */
private fun openOnThisPhone(
    context: android.content.Context,
    rowId: String,
    attachment: MessageAttachments.Attachment,
): String {
    val directory = java.io.File(context.cacheDir, "attachments/" + SafeFilename.of(rowId, "row"))
    directory.mkdirs()
    val file = java.io.File(directory, SafeFilename.of(attachment.filename))
    return runCatching {
        file.writeBytes(attachment.bytes)
        val uri = androidx.core.content.FileProvider.getUriForFile(
            context,
            context.packageName + ".files",
            file,
        )
        val intent = android.content.Intent(android.content.Intent.ACTION_VIEW)
            .setDataAndType(uri, attachment.mimeType)
            .addFlags(
                android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    android.content.Intent.FLAG_ACTIVITY_NEW_TASK,
            )
        context.startActivity(intent)
        ""
    }.getOrElse {
        // **And say so when nothing can open it.** A swallowed
        // ActivityNotFoundException is a tap that does nothing at all,
        // which reads as a broken button rather than as a phone with no
        // PDF reader on it.
        "No app on this phone opens ${attachment.filename}."
    }
}
