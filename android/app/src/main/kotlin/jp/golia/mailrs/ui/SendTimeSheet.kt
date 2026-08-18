package jp.golia.mailrs.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.SendSchedule

/**
 * When the message should leave.
 *
 * A bottom sheet, which is where Android puts a short list of choices
 * that answer one question — not a dialog, which is for something that
 * has to be decided before anything else can happen. Long-pressing Send
 * opens it; every other press means now.
 *
 * The choices are all relative and all in the future. "Later today" is
 * three hours on rather than an evening clock time, because chosen at
 * 11pm an evening has already gone — and the handler answers 400 for a
 * time that has passed, so a choice that can produce one is a choice
 * that can lose a message.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendTimeSheet(onDismiss: () -> Unit, onPick: (SendSchedule) -> Unit) {
    val theme = LocalTheme.current
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = theme.surface,
    ) {
        Column(Modifier.fillMaxWidth().padding(bottom = 24.dp).testTag("sheet.sendTime")) {
            Text(
                "Send",
                color = theme.fgMuted,
                fontSize = 12.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(start = 20.dp, bottom = 4.dp),
            )
            for (choice in SendSchedule.entries) {
                Text(
                    choice.label,
                    color = theme.fg,
                    fontSize = 15.sp,
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { onPick(choice) }
                        .padding(horizontal = 20.dp, vertical = 14.dp)
                        .testTag("sendTime.${choice.name}"),
                )
            }
        }
    }
}
