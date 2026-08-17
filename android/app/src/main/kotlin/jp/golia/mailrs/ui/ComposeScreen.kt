package jp.golia.mailrs.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Row
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Close
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.clickable
import jp.golia.mailrs.wire.RecipientAutocomplete
import jp.golia.mailrs.RecipientField
import jp.golia.mailrs.UiState
import jp.golia.mailrs.attach
import jp.golia.mailrs.cancelCompose
import jp.golia.mailrs.clearSuggestions
import jp.golia.mailrs.detach
import jp.golia.mailrs.editDraft
import jp.golia.mailrs.send
import jp.golia.mailrs.suggestContacts
import jp.golia.mailrs.MailViewModel

/**
 * Writing a message.
 *
 * **Typing belongs at the top.** `ios/DESIGN.md`: a `Form` spends a
 * section header, a card and two paddings on each of To and Subject,
 * which puts the editor three hundred points down — under the keyboard,
 * which is where it is needed. One compact line per field, and the
 * editor gets everything that is left.
 *
 * **Every field is named, including the largest one.** A composer whose
 * To and Subject are labelled and whose body is an unlabelled rectangle
 * has failed the person who came to write in it. The ghost sits behind
 * the editor and takes no touches.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ComposeScreen(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val draft = state.composing ?: return

    // **No local copy of the text.** It lives on the draft in the view
    // model, because the back gesture cancels through the shell — which
    // cannot see a screen's local variables — and leaving by the gesture
    // everybody uses would otherwise throw the message away.
    val to = draft.to
    val cc = draft.cc
    val bcc = draft.bcc
    val subject = draft.subject
    val body = draft.body
    // `OpenMultipleDocuments`, not `GetContent`: the document picker
    // reaches every provider on the phone — Drive, Files, the camera
    // roll — where `GetContent` is whichever app claims the MIME type,
    // and it hands back a URI this app is granted to read.
    val picker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments(),
    ) { uris -> vm.attach(uris) }

    // Hidden until wanted, because most mail has neither and two empty
    // lines above the subject cost every message to serve a few. Shown
    // from the start when a reopened draft already has one.
    var extraLines by remember(draft.id) { mutableStateOf(cc.isNotBlank() || bcc.isNotBlank()) }

    Column(
        Modifier
            .fillMaxSize()
            .background(theme.bg)
            .imePadding(),
    ) {
        TopAppBar(
            title = { Text(if (draft.inReplyTo != null) "Reply" else "New message", fontSize = 17.sp) },
            colors = TopAppBarDefaults.topAppBarColors(
                containerColor = theme.bg,
                titleContentColor = theme.fg,
            ),
            navigationIcon = {
                TextButton(onClick = { vm.cancelCompose() }, modifier = Modifier.testTag("button.cancel")) {
                    Text("Cancel", color = theme.accent, fontSize = 14.sp)
                }
            },
            actions = {
                IconButton(
                    onClick = { picker.launch(arrayOf("*/*")) },
                    modifier = Modifier.testTag("button.attach"),
                ) {
                    Icon(Icons.Filled.AttachFile, contentDescription = "Attach", tint = theme.fgSecondary)
                }
                if (state.sending) {
                    CircularProgressIndicator(Modifier.padding(end = 16.dp).size(20.dp), color = theme.accent)
                } else {
                    IconButton(
                        onClick = { vm.send() },
                        // The same rule the send uses, not a second one.
                        enabled = vm.recipientsIn(to).isNotEmpty(),
                        modifier = Modifier.testTag("button.send"),
                    ) {
                        Icon(
                            Icons.AutoMirrored.Filled.Send,
                            contentDescription = "Send",
                            tint = if (vm.recipientsIn(to).isNotEmpty()) theme.accent else theme.fgMuted,
                        )
                    }
                }
            },
        )

        CompactField(
            label = "To",
            value = to,
            tag = "field.to",
            trailing = {
                if (!extraLines) {
                    TextButton(
                        onClick = { extraLines = true },
                        modifier = Modifier.testTag("button.ccBcc"),
                    ) {
                        Text("Cc/Bcc", color = theme.accent, fontSize = 12.sp)
                    }
                }
            },
        ) {
            vm.editDraft(to = it)
            vm.suggestContacts(RecipientField.To, it)
        }
        Suggestions(state, RecipientField.To, vm) { picked ->
            vm.editDraft(to = RecipientAutocomplete.completing(to, picked))
        }
        HorizontalDivider(color = theme.border, thickness = 0.5.dp)

        if (extraLines) {
            CompactField("Cc", cc, "field.cc") {
                vm.editDraft(cc = it)
                vm.suggestContacts(RecipientField.Cc, it)
            }
            Suggestions(state, RecipientField.Cc, vm) { picked ->
                vm.editDraft(cc = RecipientAutocomplete.completing(cc, picked))
            }
            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
            // Bcc says what it does. "Blind" is the whole point and the
            // reason it is worth a word: a reader who confuses it with
            // Cc has told a mailing list who else is on it.
            CompactField("Bcc", bcc, "field.bcc") {
                vm.editDraft(bcc = it)
                vm.suggestContacts(RecipientField.Bcc, it)
            }
            Suggestions(state, RecipientField.Bcc, vm) { picked ->
                vm.editDraft(bcc = RecipientAutocomplete.completing(bcc, picked))
            }
            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
        }

        CompactField("Subject", subject, "field.subject") { vm.editDraft(subject = it) }
        HorizontalDivider(color = theme.border, thickness = 0.5.dp)

        if (draft.attachments.isNotEmpty()) {
            Column(Modifier.fillMaxWidth().testTag("list.draftAttachments")) {
                for (a in draft.attachments) {
                    Row(
                        Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            Icons.Filled.AttachFile,
                            contentDescription = null,
                            tint = theme.fgMuted,
                            modifier = Modifier.size(15.dp),
                        )
                        Text(
                            a.filename,
                            color = theme.fg,
                            fontSize = 13.sp,
                            maxLines = 1,
                            modifier = Modifier.weight(1f).padding(start = 8.dp).testTag("row.draftAttachment"),
                        )
                        Text(humanSize(a.size), color = theme.fgMuted, fontSize = 11.sp)
                        IconButton(
                            onClick = { vm.detach(a) },
                            modifier = Modifier.testTag("button.detach"),
                        ) {
                            Icon(
                                Icons.Filled.Close,
                                contentDescription = "Remove ${a.filename}",
                                tint = theme.fgMuted,
                                modifier = Modifier.size(15.dp),
                            )
                        }
                    }
                }
            }
            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
        }

        if (state.error != null) {
            Text(
                state.error,
                color = theme.danger,
                fontSize = 13.sp,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("text.sendError"),
            )
        }

        // The editor takes what is left, and its placeholder sits behind
        // it rather than inside — a text field with a value cannot show
        // one, and an unlabelled rectangle is the defect this avoids.
        Box(Modifier.weight(1f).fillMaxWidth()) {
            if (body.isEmpty()) {
                Text(
                    "Message",
                    color = theme.fgMuted,
                    fontSize = 15.sp,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                )
            }
            BasicTextField(
                value = body,
                onValueChange = { vm.editDraft(body = it) },
                textStyle = TextStyle(color = theme.fg, fontSize = 15.sp),
                cursorBrush = SolidColor(theme.accent),
                modifier = Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 12.dp)
                    .testTag("field.body"),
            )
        }
    }
}

/** One line per field: a label, then the value, on the same row. */
@Composable
private fun CompactField(
    label: String,
    value: String,
    tag: String,
    trailing: @Composable () -> Unit = {},
    onChange: (String) -> Unit,
) {
    val theme = LocalTheme.current
    androidx.compose.foundation.layout.Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = theme.fgMuted, fontSize = 14.sp, modifier = Modifier.padding(end = 10.dp))
        BasicTextField(
            value = value,
            onValueChange = onChange,
            singleLine = true,
            textStyle = TextStyle(color = theme.fg, fontSize = 15.sp),
            cursorBrush = SolidColor(theme.accent),
            modifier = Modifier.weight(1f).testTag(tag),
        )
        trailing()
    }
}

/**
 * Contacts for the line being typed, under that line.
 *
 * Under the field it belongs to rather than in one shared list: a
 * suggestion that could land in the wrong recipient line is how a
 * message goes to somebody who was never meant to see it.
 */
@Composable
private fun Suggestions(
    state: UiState,
    field: RecipientField,
    vm: MailViewModel,
    onPick: (String) -> Unit,
) {
    val theme = LocalTheme.current
    if (state.suggestingFor != field) return
    for (contact in state.suggestions.take(4)) {
        Text(
            contact,
            color = theme.fgSecondary,
            fontSize = 13.sp,
            modifier = Modifier
                .fillMaxWidth()
                .clickable {
                    onPick(contact)
                    vm.clearSuggestions()
                }
                .padding(horizontal = 16.dp, vertical = 10.dp)
                .testTag("suggestion.contact"),
        )
    }
}
