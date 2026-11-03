package jp.golia.mailrs.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.background
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.DeleteForever
import androidx.compose.material.icons.automirrored.filled.Label
import androidx.compose.material.icons.automirrored.filled.LabelOff
import androidx.compose.material.icons.filled.LocalOffer
import androidx.compose.material.icons.filled.MarkEmailRead
import androidx.compose.material.icons.filled.MarkEmailUnread
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.MoveToInbox
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.filled.Report
import androidx.compose.material.icons.filled.ReportOff
import androidx.compose.material.icons.filled.RestoreFromTrash
import androidx.compose.material.icons.filled.Snooze
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.StarBorder
import androidx.compose.material.icons.filled.Unarchive
import androidx.compose.material.icons.filled.VerifiedUser
import androidx.compose.material.icons.filled.WbSunny
import androidx.compose.material.icons.outlined.PushPin
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.R
import jp.golia.mailrs.wire.RowAction
import jp.golia.mailrs.wire.WireTriage

/** The icon an action has everywhere it is offered — the web's choices, in Material's names. */
fun iconOf(action: RowAction): ImageVector = when (action) {
    RowAction.Archive -> Icons.Filled.Archive
    RowAction.Unarchive -> Icons.Filled.Unarchive
    RowAction.Trash -> Icons.Filled.Delete
    RowAction.Untrash -> Icons.Filled.RestoreFromTrash
    RowAction.DeleteForever -> Icons.Filled.DeleteForever
    RowAction.MarkJunk -> Icons.Filled.Report
    RowAction.MarkNotJunk -> Icons.Filled.ReportOff
    RowAction.MarkNotification -> Icons.Filled.Notifications
    RowAction.MarkPromotion -> Icons.Filled.LocalOffer
    RowAction.MoveToInbox -> Icons.Filled.MoveToInbox
    RowAction.Pin -> Icons.Filled.PushPin
    RowAction.Unpin -> Icons.Outlined.PushPin
    RowAction.Snooze -> Icons.Filled.Snooze
    RowAction.Unsnooze -> Icons.Filled.WbSunny
    RowAction.Read -> Icons.Filled.MarkEmailRead
    RowAction.Unread -> Icons.Filled.MarkEmailUnread
    RowAction.Star -> Icons.Filled.Star
    RowAction.Unstar -> Icons.Filled.StarBorder
    RowAction.Label -> Icons.AutoMirrored.Filled.Label
    RowAction.Unlabel -> Icons.AutoMirrored.Filled.LabelOff
    RowAction.Release -> Icons.Filled.VerifiedUser
}

/**
 * The overflow of a top bar: every action [actions] lists, in that order.
 * Labelling opens [onLabel] instead, because it needs a label chosen.
 */
@Composable
fun RowActionMenu(
    actions: List<RowAction>,
    tag: String,
    onPick: (RowAction) -> Unit,
) {
    val theme = LocalTheme.current
    var open by rememberSaveable { mutableStateOf(false) }
    IconButton(onClick = { open = true }, modifier = Modifier.testTag(tag)) {
        Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.common_more), tint = theme.fgSecondary)
    }
    DropdownMenu(expanded = open, onDismissRequest = { open = false }, containerColor = theme.surface) {
        for (action in actions) {
            DropdownMenuItem(
                text = { Text(stringResource(action.label), color = theme.fg, fontSize = 14.sp) },
                leadingIcon = { Icon(iconOf(action), contentDescription = null, tint = theme.fgSecondary) },
                onClick = {
                    open = false
                    onPick(action)
                },
                modifier = Modifier.testTag("action." + action.name),
            )
        }
    }
}

/** Choose a label to put on what is selected or open. */
@Composable
fun LabelPickerDialog(
    labels: List<WireTriage.Label>,
    onPick: (WireTriage.Label) -> Unit,
    onDismiss: () -> Unit,
) {
    val theme = LocalTheme.current
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = theme.surface,
        title = { Text(stringResource(R.string.label_picker_title), color = theme.fg) },
        text = {
            Column(Modifier.testTag("dialog.labels")) {
                when (labels.isEmpty()) {
                    true -> Text(stringResource(R.string.label_picker_none), color = theme.fgSecondary, fontSize = 14.sp)
                    false -> for (l in labels) {
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .clickable { onPick(l) }
                                .padding(vertical = 10.dp)
                                .testTag("label." + l.id),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            LabelDot(l.color)
                            Text(l.name, color = theme.fg, fontSize = 15.sp, modifier = Modifier.padding(start = 12.dp))
                        }
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.common_cancel)) }
        },
    )
}

/** Deleting for good is the one action nothing takes back, so it asks. */
@Composable
fun DeleteForeverDialog(count: Int, onAnswer: (Boolean) -> Unit) {
    val theme = LocalTheme.current
    AlertDialog(
        onDismissRequest = { onAnswer(false) },
        containerColor = theme.surface,
        title = { Text(stringResource(R.string.delete_forever_title), color = theme.fg) },
        text = { Text(stringResource(R.string.delete_forever_body, count), color = theme.fgSecondary) },
        confirmButton = {
            TextButton(onClick = { onAnswer(true) }, modifier = Modifier.testTag("button.deleteForever")) {
                Text(stringResource(R.string.action_delete_forever), color = theme.danger)
            }
        },
        dismissButton = {
            TextButton(onClick = { onAnswer(false) }) { Text(stringResource(R.string.common_cancel)) }
        },
    )
}

/** A label's colour, as the web shows it beside the name. */
@Composable
fun LabelDot(color: String) {
    Row(
        Modifier
            .size(10.dp)
            .background(labelColor(color), CircleShape),
    ) {}
}

/** The server's label colours (`LABEL_COLORS`), in this app's palette. */
private fun labelColor(name: String): androidx.compose.ui.graphics.Color = when (name) {
    "red" -> androidx.compose.ui.graphics.Color(0xFFE5484D)
    "orange" -> androidx.compose.ui.graphics.Color(0xFFF76B15)
    "yellow" -> androidx.compose.ui.graphics.Color(0xFFFFC53D)
    "green" -> androidx.compose.ui.graphics.Color(0xFF30A46C)
    "teal" -> androidx.compose.ui.graphics.Color(0xFF12A594)
    "blue" -> androidx.compose.ui.graphics.Color(0xFF3B7DDD)
    "purple" -> androidx.compose.ui.graphics.Color(0xFF8E4EC6)
    else -> androidx.compose.ui.graphics.Color(0xFF8D8D98)
}
