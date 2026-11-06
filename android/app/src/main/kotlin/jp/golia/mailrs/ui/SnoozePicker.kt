package jp.golia.mailrs.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.R
import jp.golia.mailrs.wire.SnoozeChoice
import java.time.ZonedDateTime

/**
 * Until when: the iPhone's four answers to one question, asked once the
 * menu's Snooze is chosen. [onPick] gets the end in epoch seconds, read
 * in the phone's zone at the moment of the choice.
 */
@Composable
fun SnoozePickerDialog(onPick: (Long) -> Unit, onDismiss: () -> Unit) {
    val theme = LocalTheme.current
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = theme.surface,
        title = { Text(stringResource(R.string.action_snooze), color = theme.fg) },
        text = {
            Column(Modifier.testTag("dialog.snooze")) {
                for (choice in SnoozeChoice.entries) {
                    Text(
                        choice.label,
                        color = theme.fg,
                        fontSize = 15.sp,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onPick(choice.fireDate(ZonedDateTime.now())) }
                            .padding(vertical = 12.dp)
                            .testTag("snooze." + choice.name),
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.common_cancel)) }
        },
    )
}
