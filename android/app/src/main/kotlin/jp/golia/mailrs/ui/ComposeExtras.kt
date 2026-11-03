package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.filled.FormatBold
import androidx.compose.material.icons.filled.FormatItalic
import androidx.compose.material.icons.filled.Link
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
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
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.SentNote
import jp.golia.mailrs.UiState
import jp.golia.mailrs.sentNoticeShown
import jp.golia.mailrs.undoSend
import jp.golia.mailrs.wire.MarkdownFormat
import jp.golia.mailrs.wire.SendHold
import jp.golia.mailrs.wire.Template
import jp.golia.mailrs.wire.Texts
import kotlinx.coroutines.withTimeoutOrNull
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * The From line, shown only when there is more than one address to leave by
 * — the signed-in one and its `send_as`, as on the web.
 */
@Composable
fun FromLine(options: List<String>, selected: String, onPick: (String) -> Unit) {
    if (options.size < 2) return
    val theme = LocalTheme.current
    var open by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(Texts.get(R.string.compose_from), color = theme.fgMuted, fontSize = 14.sp, modifier = Modifier.padding(end = 10.dp))
        Box(Modifier.weight(1f)) {
            TextButton(onClick = { open = true }, modifier = Modifier.testTag("field.from")) {
                Text(selected.ifEmpty { options.first() }, color = theme.fg, fontSize = 15.sp)
            }
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                for (address in options) {
                    DropdownMenuItem(
                        text = { Text(address) },
                        onClick = {
                            open = false
                            onPick(address)
                        },
                        modifier = Modifier.testTag("from.$address"),
                    )
                }
            }
        }
    }
    HorizontalDivider(color = theme.border, thickness = 0.5.dp)
}

/** The account's templates, as the web composer lists them; absent when there are none. */
@Composable
fun TemplateMenu(templates: List<Template>, onPick: (Template) -> Unit) {
    if (templates.isEmpty()) return
    val theme = LocalTheme.current
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.testTag("button.templates")) {
            Icon(Icons.Filled.ContentPaste, contentDescription = Texts.get(R.string.compose_templates), tint = theme.fgSecondary)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (t in templates) {
                DropdownMenuItem(
                    text = { Text(t.name) },
                    onClick = {
                        open = false
                        onPick(t)
                    },
                    modifier = Modifier.testTag("template.${t.id}"),
                )
            }
        }
    }
}

/** "Scheduled for …" on a resumed draft that was set to go later, with a way back to now. */
@Composable
fun ScheduledLine(at: Long?, onClear: () -> Unit) {
    if (at == null) return
    val theme = LocalTheme.current
    val shown = DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
        .format(Instant.ofEpochSecond(at).atZone(ZoneId.systemDefault()))
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            Texts.get(R.string.compose_scheduled_for, shown),
            color = theme.fgSecondary,
            fontSize = 13.sp,
            modifier = Modifier.weight(1f).testTag("text.scheduled"),
        )
        TextButton(onClick = onClear, modifier = Modifier.testTag("button.unschedule")) {
            Text(Texts.get(R.string.compose_send_now_instead), color = theme.accent, fontSize = 13.sp)
        }
    }
}

/**
 * The message body, with the bold, italic and link row the web composer has,
 * writing the same Markdown marks over the selection.
 *
 * The text lives on the draft in the view model; the selection lives here.
 * When the text changes from outside (a template applied), the caret goes to
 * its end.
 */
@Composable
fun MessageBodyEditor(body: String, onChange: (String) -> Unit, modifier: Modifier = Modifier) {
    val theme = LocalTheme.current
    var field by remember { mutableStateOf(TextFieldValue(body, TextRange(body.length))) }
    LaunchedEffect(body) {
        if (field.text != body) field = TextFieldValue(body, TextRange(body.length))
    }
    fun format(kind: MarkdownFormat.Kind) {
        val edit = MarkdownFormat.apply(field.text, field.selection.min, field.selection.max, kind)
        field = TextFieldValue(edit.value, TextRange(edit.start, edit.end))
        onChange(edit.value)
    }
    Column(modifier) {
        Row(Modifier.padding(horizontal = 4.dp)) {
            IconButton(onClick = { format(MarkdownFormat.Kind.Bold) }, modifier = Modifier.testTag("button.format.bold")) {
                Icon(Icons.Filled.FormatBold, contentDescription = Texts.get(R.string.format_bold), tint = theme.fgSecondary)
            }
            IconButton(onClick = { format(MarkdownFormat.Kind.Italic) }, modifier = Modifier.testTag("button.format.italic")) {
                Icon(Icons.Filled.FormatItalic, contentDescription = Texts.get(R.string.format_italic), tint = theme.fgSecondary)
            }
            IconButton(onClick = { format(MarkdownFormat.Kind.Link) }, modifier = Modifier.testTag("button.format.link")) {
                Icon(Icons.Filled.Link, contentDescription = Texts.get(R.string.format_link), tint = theme.fgSecondary)
            }
        }
        // The editor takes what is left, and its placeholder sits behind it
        // rather than inside — a text field with a value cannot show one.
        Box(Modifier.weight(1f).fillMaxWidth()) {
            if (field.text.isEmpty()) {
                Text(
                    Texts.get(R.string.compose_message),
                    color = theme.fgMuted,
                    fontSize = 15.sp,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                )
            }
            BasicTextField(
                value = field,
                onValueChange = { next ->
                    field = next
                    onChange(next.text)
                },
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

/**
 * "Message sent — Undo" while the server holds the send (`undo_seconds`), then
 * how the Undo went: cancelled, or too late because it had already left.
 */
@Composable
fun SentSnackbar(state: UiState, vm: MailViewModel, snackbars: SnackbarHostState) {
    LaunchedEffect(state.sent) {
        val note = state.sent ?: return@LaunchedEffect
        when (note.undo) {
            SentNote.Undo.Offered -> {
                val result = withTimeoutOrNull(SendHold.OFFERED_MS) {
                    snackbars.showSnackbar(
                        message = Texts.get(R.string.compose_message_sent),
                        actionLabel = Texts.get(R.string.common_undo),
                        duration = SnackbarDuration.Indefinite,
                    )
                }
                when (result == SnackbarResult.ActionPerformed) {
                    true -> vm.undoSend()
                    false -> vm.sentNoticeShown()
                }
            }
            SentNote.Undo.Cancelling -> Unit
            SentNote.Undo.Cancelled -> {
                snackbars.showSnackbar(Texts.get(R.string.compose_send_cancelled))
                vm.sentNoticeShown()
            }
            SentNote.Undo.TooLate -> {
                snackbars.showSnackbar(Texts.get(R.string.compose_undo_too_late))
                vm.sentNoticeShown()
            }
        }
    }
}
