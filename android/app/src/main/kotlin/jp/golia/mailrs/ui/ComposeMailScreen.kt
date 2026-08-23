package jp.golia.mailrs.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.accounts.AccountColour
import jp.golia.mailrs.accounts.AccountSender
import jp.golia.mailrs.accounts.AccountStore
import jp.golia.mailrs.accounts.MailAccount
import jp.golia.mailrs.accounts.OutgoingMessage
import jp.golia.mailrs.accounts.PickedFile
import jp.golia.mailrs.accounts.ReadFiles
import kotlinx.coroutines.launch

/**
 * Writing a message from a connected mailbox.
 *
 * The From row is a picker even with one account, because which address
 * a message leaves by is the thing people get wrong and the thing they
 * cannot see afterwards. It is at the top, where every mail client puts
 * it.
 */
@Composable
fun ComposeMailScreen(
    accounts: List<MailAccount>,
    initial: OutgoingMessage.Draft,
    initialAccountId: String,
    onClose: () -> Unit,
) {
    val theme = LocalTheme.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val store = remember { AccountStore(context) }

    var from by remember {
        mutableStateOf(
            accounts.firstOrNull { it.id == initialAccountId } ?: accounts.firstOrNull(),
        )
    }
    var to by remember { mutableStateOf(initial.to.joinToString(", ")) }
    var cc by remember { mutableStateOf(initial.cc.joinToString(", ")) }
    var bcc by remember { mutableStateOf("") }
    // Collapsed until asked for: most messages have neither, and two
    // empty boxes above the subject is two more things to read past
    // every time. Opened already if the draft arrived with a Cc — a
    // reply-all that hides what it is copying is worse than a box.
    var showCopies by remember { mutableStateOf(initial.cc.isNotEmpty()) }
    var subject by remember { mutableStateOf(initial.subject) }
    var body by remember { mutableStateOf(initial.body) }
    var sending by remember { mutableStateOf(false) }
    var picked by remember { mutableStateOf(emptyList<PickedFile>()) }

    // `OpenMultipleDocuments`, not `GetContent`: the document picker
    // reaches every provider on the phone — Drive, Files, the camera
    // roll — where `GetContent` is whichever app claims the MIME type,
    // and it hands back a URI this app is granted to read. The same
    // choice the app's own composer makes, for the same reason.
    val picker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenMultipleDocuments(),
    ) { uris ->
        picked = picked + uris.mapNotNull { describe(context, it) }
    }
    var failure by remember { mutableStateOf("") }

    BackHandler { onClose() }

    fun send() {
        val account = from ?: return
        if (sending) return
        sending = true
        failure = ""
        scope.launch {
            fun addresses(text: String) =
                text.split(",").map { it.trim() }.filter { it.isNotEmpty() }
            val files = readAll(context, picked)
            val draft = initial.copy(
                from = account.address,
                fromName = account.displayName,
                to = addresses(to),
                cc = addresses(cc),
                subject = subject,
                body = body,
                attachments = files.attachments,
            )
            // Bcc goes to the sender as the envelope's extra
            // recipients and never into the headers — that is what
            // makes a blind copy blind.
            // **Nothing is sent while a file is missing.** A message
            // that goes without the attachment it was written around
            // is worse than one that does not go: the second can be
            // sent again, and the first has already arrived looking
            // complete.
            if (files.lost.isNotEmpty()) {
                failure = "Could not read " + files.lost.joinToString(", ") +
                    ". Nothing was sent."
                sending = false
                return@launch
            }
            when (val outcome = AccountSender.send(draft, account, store, addresses(bcc))) {
                is AccountSender.Outcome.Sent -> onClose()
                is AccountSender.Outcome.Failed -> {
                    failure = outcome.why
                    sending = false
                }
            }
        }
    }

    Column(
        Modifier.fillMaxSize().background(theme.bg).verticalScroll(rememberScrollState()),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(onClick = onClose, modifier = Modifier.testTag("compose.cancel")) {
                Text("Cancel", color = theme.accent, fontSize = 13.sp)
            }
            Box(Modifier.weight(1f))
            when {
                sending -> CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else -> TextButton(
                    onClick = { send() },
                    modifier = Modifier.testTag("compose.send"),
                ) {
                    Text("Send", color = theme.accent, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }

        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("From", color = theme.fgMuted, fontSize = 11.sp)
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                for (account in accounts) {
                    FromChip(account, account.id == from?.id) { from = account }
                }
            }

            OutlinedTextField(
                value = to,
                onValueChange = { to = it },
                label = { Text("To") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("compose.to"),
            )
            if (showCopies) {
                OutlinedTextField(
                    value = cc,
                    onValueChange = { cc = it },
                    label = { Text("Cc") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("compose.cc"),
                )
                OutlinedTextField(
                    value = bcc,
                    onValueChange = { bcc = it },
                    label = { Text("Bcc") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("compose.bcc"),
                )
            } else {
                TextButton(
                    onClick = { showCopies = true },
                    modifier = Modifier.testTag("compose.showCopies"),
                ) {
                    Text("Cc / Bcc", color = theme.accent, fontSize = 12.sp)
                }
            }
            OutlinedTextField(
                value = subject,
                onValueChange = { subject = it },
                label = { Text("Subject") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("compose.subject"),
            )
            OutlinedTextField(
                value = body,
                onValueChange = { body = it },
                label = { Text("Message") },
                modifier = Modifier.fillMaxWidth().heightIn(min = 200.dp).testTag("compose.body"),
            )

            TextButton(
                onClick = { picker.launch(arrayOf("*/*")) },
                modifier = Modifier.testTag("compose.attach"),
            ) {
                Text("Attach a file", color = theme.accent, fontSize = 12.sp)
            }
            for (file in picked) {
                Row(
                    Modifier.fillMaxWidth().padding(vertical = 2.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(file.filename, color = theme.fg, fontSize = 12.sp)
                    Text(humanSize(file.size), color = theme.fgMuted, fontSize = 11.sp)
                    Box(Modifier.weight(1f))
                    TextButton(onClick = { picked = picked - file }) {
                        Text("Remove", color = theme.fgMuted, fontSize = 11.sp)
                    }
                }
            }

            if (failure.isNotEmpty()) {
                // In the screen, not a dialog that has to be dismissed
                // before the message can be fixed — what went wrong and
                // what to change are the same screen.
                Text(
                    failure,
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                    modifier = Modifier.testTag("compose.failure"),
                )
            }
        }
    }
}

@Composable
private fun FromChip(account: MailAccount, on: Boolean, onTap: () -> Unit) {
    val theme = LocalTheme.current
    val background = when {
        on -> theme.accent.copy(alpha = 0.18f)
        else -> theme.bgSecondary
    }
    Row(
        Modifier
            .clip(RoundedCornerShape(14.dp))
            .background(background)
            .clickable { onTap() }
            .padding(horizontal = 10.dp, vertical = 6.dp)
            .testTag("compose.from.${account.address}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(
            Modifier.size(8.dp).clip(CircleShape)
                .background(
                    Color(android.graphics.Color.parseColor(AccountColour.forId(account.id))),
                ),
        )
        Text(account.address, color = theme.fg, fontSize = 12.sp)
    }
}

/**
 * What a picked URI is called and how big it is.
 *
 * From the content resolver rather than the URI's last path segment: a
 * document provider's URI is an opaque id, and `msf:1000000042` is not
 * a filename anybody wants to see arrive in their mail. The same
 * reasoning as the app's own composer, and the same source.
 */
private fun describe(context: android.content.Context, uri: android.net.Uri): PickedFile? =
    runCatching {
        context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            val nameAt = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
            val sizeAt = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
            if (!cursor.moveToFirst()) return@use null
            PickedFile(
                uri = uri.toString(),
                filename = when {
                    nameAt >= 0 -> cursor.getString(nameAt)
                    else -> "attachment"
                },
                size = when {
                    sizeAt >= 0 && !cursor.isNull(sizeAt) -> cursor.getLong(sizeAt)
                    else -> 0L
                },
                mimeType = context.contentResolver.getType(uri) ?: "application/octet-stream",
            )
        }
    }.getOrNull()

/**
 * The bytes, read once, at the moment of sending.
 *
 * A file that cannot be read is **named** rather than dropped: picking
 * three files and sending two, with nothing said, is somebody being
 * quietly lied to about what went out.
 */
private fun readAll(context: android.content.Context, files: List<PickedFile>): ReadFiles {
    val attachments = mutableListOf<OutgoingMessage.Attachment>()
    val lost = mutableListOf<String>()
    for (file in files) {
        val bytes = runCatching {
            context.contentResolver.openInputStream(android.net.Uri.parse(file.uri))
                ?.use { it.readBytes() }
        }.getOrNull()
        when (bytes) {
            null -> lost.add(file.filename)
            else -> attachments.add(
                OutgoingMessage.Attachment(file.filename, file.mimeType, bytes),
            )
        }
    }
    return ReadFiles(attachments, lost)
}
