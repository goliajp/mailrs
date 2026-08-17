package jp.golia.mailrs.ui

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
fun ComposeScreen(state: MailViewModel.UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val draft = state.composing ?: return

    var to by remember(draft.id) { mutableStateOf(draft.to.joinToString(", ")) }
    var subject by remember(draft.id) { mutableStateOf(draft.subject) }
    var body by remember(draft.id) { mutableStateOf(draft.body) }

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
                if (state.sending) {
                    CircularProgressIndicator(Modifier.padding(end = 16.dp).size(20.dp), color = theme.accent)
                } else {
                    IconButton(
                        onClick = { vm.send(to, subject, body) },
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

        CompactField("To", to, "field.to") { to = it }
        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
        CompactField("Subject", subject, "field.subject") { subject = it }
        HorizontalDivider(color = theme.border, thickness = 0.5.dp)

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
                onValueChange = { body = it },
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
private fun CompactField(label: String, value: String, tag: String, onChange: (String) -> Unit) {
    val theme = LocalTheme.current
    androidx.compose.foundation.layout.Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = theme.fgMuted, fontSize = 14.sp, modifier = Modifier.padding(end = 10.dp))
        BasicTextField(
            value = value,
            onValueChange = onChange,
            singleLine = true,
            textStyle = TextStyle(color = theme.fg, fontSize = 15.sp),
            cursorBrush = SolidColor(theme.accent),
            modifier = Modifier.fillMaxWidth().testTag(tag),
        )
    }
}
