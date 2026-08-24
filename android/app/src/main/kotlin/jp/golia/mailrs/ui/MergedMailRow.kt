package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.accounts.AccountColour
import jp.golia.mailrs.accounts.MailAccount
import jp.golia.mailrs.accounts.MailboxRow

/**
 * One message in the merged list, and the gestures on it.
 *
 * Split from [MergedMailScreen] by subject rather than by size: that
 * file answers "what is on this screen and what does it do", and this
 * one answers "what does one row look like and what happens when it is
 * swiped". The two change for different reasons.
 */
/**
 * One message in the merged list.
 *
 * Swiping is the gesture every mail client uses, and the two directions
 * are chosen the way they are everywhere: **the destructive one is the
 * one nobody reaches for by accident**, and the reversible one is on
 * the side a thumb rests.
 */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
internal fun MergedMailRow(
    row: MailboxRow,
    account: MailAccount?,
    onTap: () -> Unit,
    onDelete: () -> Unit,
    onMarkUnread: () -> Unit,
) {
    val state = androidx.compose.material3.rememberSwipeToDismissBoxState()
    // Driven from the settled value rather than from a veto callback:
    // the callback overload is deprecated, and vetoing was the wrong
    // shape anyway — the row goes when the **server** says it has, and
    // one that vanishes before that comes back on the next fetch
    // looking like a bug. So the swipe springs back and the list
    // updates when the action lands.
    LaunchedEffect(state.currentValue) {
        when (state.currentValue) {
            androidx.compose.material3.SwipeToDismissBoxValue.EndToStart -> {
                onDelete()
                state.reset()
            }
            androidx.compose.material3.SwipeToDismissBoxValue.StartToEnd -> {
                onMarkUnread()
                state.reset()
            }
            else -> Unit
        }
    }
    androidx.compose.material3.SwipeToDismissBox(
        state = state,
        backgroundContent = { SwipeBackground(state.dismissDirection) },
        modifier = Modifier.testTag("mail.swipe.${row.id}"),
    ) {
        MergedMailRowBody(row, account, onTap)
    }
}

@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun SwipeBackground(direction: androidx.compose.material3.SwipeToDismissBoxValue) {
    val theme = LocalTheme.current
    val text = when (direction) {
        androidx.compose.material3.SwipeToDismissBoxValue.EndToStart -> "Delete"
        androidx.compose.material3.SwipeToDismissBoxValue.StartToEnd -> "Unread"
        else -> ""
    }
    val alignment = when (direction) {
        androidx.compose.material3.SwipeToDismissBoxValue.EndToStart -> Alignment.CenterEnd
        else -> Alignment.CenterStart
    }
    Box(
        Modifier.fillMaxWidth().background(theme.bgSecondary).padding(horizontal = 20.dp),
        contentAlignment = alignment,
    ) {
        Text(text, color = theme.fgMuted, fontSize = 12.sp)
    }
}

@Composable
private fun MergedMailRowBody(row: MailboxRow, account: MailAccount?, onTap: () -> Unit) {
    val theme = LocalTheme.current
    // Unread is heavier. The only thing on the row that says so without
    // colour, which is why it is weight and not a tint.
    val weight = when {
        row.seen -> FontWeight.Normal
        else -> FontWeight.SemiBold
    }
    val subjectColour = when {
        row.seen -> theme.fgMuted
        else -> theme.fg
    }
    Row(
        Modifier.fillMaxWidth().clickable { onTap() }
            .padding(horizontal = 16.dp, vertical = 8.dp)
            .testTag("mail.row.${row.id}"),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(
            Modifier.size(8.dp).clip(CircleShape)
                .background(Color(android.graphics.Color.parseColor(AccountColour.forId(row.accountId)))),
        )
        Column {
            Text(row.displaySender, color = theme.fg, fontSize = 14.sp, fontWeight = weight)
            Text(row.displaySubject, color = subjectColour, fontSize = 13.sp, maxLines = 2)
            // Which mailbox, in words. The dot is a shortcut for people
            // who can see it; this line is the answer for everybody else.
            Text(
                "${account?.title ?: "Unknown"} · ${row.folder}",
                color = theme.fgMuted,
                fontSize = 11.sp,
            )
        }
    }
}
