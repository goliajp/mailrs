package jp.golia.mailrs.ui

import jp.golia.mailrs.R
import jp.golia.mailrs.wire.Texts
import androidx.compose.foundation.background
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PriorityHigh
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.testTag
import jp.golia.mailrs.wire.TeamRow
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.SenderName
import jp.golia.mailrs.wire.Wire

/**
 * One conversation, as a row — the unit everything multiplies.
 *
 * The same three lines as the iPhone's row and the web's, from the same
 * wire fields:
 *
 *     ● sender +2····················×3  14:32
 *       subject····························★
 *       first line of the newest message
 *
 * - Line 1: the other participant's name (never the reader's own, never
 *   a bare address), `+N` for everyone else on it, `×N` only when N > 1,
 *   date.
 * - Line 2: subject, secondary; star when flagged.
 * - Line 3: the snippet, which says what the message wants where the
 *   subject only says what it is filed under.
 * - Read rows recede to 70% — unread already carries the dot and the
 *   weight, and dimming what is done is what makes a long list
 *   scannable rather than uniformly loud.
 *
 * **A row that wraps is a defect**, whatever it says. Every variable
 * piece is bounded to one line and the row says which yields: the name
 * gives way before the date does, because a truncated name is still a
 * name and a truncated timestamp is nothing.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ConversationRow(
    c: Wire.Conversation,
    /** The signed-in address, so the row wears somebody else's name. */
    myAddress: String,
    selected: Boolean = false,
    onLongPress: (() -> Unit)? = null,
    /**
     * Triage without a gesture.
     *
     * Archive and mark-read live on a swipe, and TalkBack takes swipes
     * over for its own navigation — so a row whose only path to filing
     * is a swipe has no path at all for the person using a screen
     * reader. Declared on this node rather than on the swipe container
     * around it, because this is the node a screen reader focuses.
     */
    onArchive: (() -> Unit)? = null,
    onMarkRead: (() -> Unit)? = null,
    onClick: () -> Unit,
) {
    val theme = LocalTheme.current
    val unread = c.unreadCount > 0
    val others = SenderName.others(c.participants, myAddress)
    // whose colour and initial the avatar wears: the participant the name comes from
    val face = others.firstOrNull() ?: c.participants.firstOrNull().orEmpty()
    val extraParticipants = maxOf(0, others.size - 1)

    Row(
        Modifier
            .fillMaxWidth()
            // A long press starts a selection, which is the gesture
            // Android has meant by "act on more than one of these" since
            // before this app existed. `combinedClickable` is what
            // carries both meanings on one row; a separate long-press
            // pointer modifier would race the tap.
            .combinedClickable(onLongClick = onLongPress, onClick = onClick)
            .background(if (selected) theme.accent.copy(alpha = 0.14f) else theme.bg)
            .testTag("row.conversation")
            .semantics {
                customActions = listOfNotNull(
                    onArchive?.let { CustomAccessibilityAction(Texts.get(R.string.action_archive)) { it(); true } },
                    onMarkRead?.let { CustomAccessibilityAction(Texts.get(R.string.row_mark_read)) { it(); true } },
                )
            }
            .padding(horizontal = 16.dp, vertical = 6.dp)
            .alpha(if (unread) 1f else 0.7f),
        verticalAlignment = Alignment.Top,
    ) {
        SenderAvatarView(face, unread = unread)

        Column(Modifier.padding(start = 12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = SenderName.rowFace(c.participants, myAddress).ifBlank { Texts.get(R.string.mail_no_sender) },
                    color = theme.fg,
                    fontSize = 15.sp,
                    fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Normal,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false).testTag("row.sender.${c.threadId}"),
                )
                if (extraParticipants > 0) {
                    Text(
                        "+$extraParticipants",
                        color = theme.fgSecondary,
                        fontSize = 12.sp,
                        maxLines = 1,
                        modifier = Modifier.padding(start = 5.dp).testTag("row.others.${c.threadId}"),
                    )
                }
                if (c.messageCount > 1) {
                    Text(
                        "×${c.messageCount}",
                        color = theme.fgMuted,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(start = 5.dp),
                    )
                }
                // The date never yields, so it sits after a spacer that
                // takes what the name gave up.
                Text(
                    RowDate.format(c.lastDate),
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                    maxLines = 1,
                    modifier = Modifier
                        .weight(1f, fill = true)
                        .padding(start = 8.dp),
                    textAlign = androidx.compose.ui.text.style.TextAlign.End,
                )
            }
            Row(
                Modifier.padding(top = 1.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (c.importanceLevel == "critical" || c.importanceLevel == "important") {
                    Icon(
                        Icons.Filled.PriorityHigh,
                        contentDescription = when (c.importanceLevel) {
                            "critical" -> Texts.get(R.string.row_critical)
                            else -> Texts.get(R.string.row_important)
                        },
                        tint = if (c.importanceLevel == "critical") theme.danger else theme.warning,
                        modifier = Modifier.size(13.dp),
                    )
                }
                Text(
                    text = c.subject.ifBlank { Texts.get(R.string.mail_no_subject) },
                    color = theme.fgSecondary,
                    fontSize = 14.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                if (c.flagged) {
                    Icon(
                        Icons.Filled.Star,
                        contentDescription = Texts.get(R.string.row_flagged),
                        tint = theme.warning,
                        modifier = Modifier.size(13.dp),
                    )
                }
                // Both directions in one thread: the web's capsule chip.
                if (c.receivedCount > 0 && c.sentCount > 0) {
                    Text(
                        "↓${c.receivedCount} ↑${c.sentCount}",
                        color = theme.fgMuted,
                        fontSize = 10.sp,
                        modifier = Modifier
                            .background(theme.bgTertiary, RoundedCornerShape(6.dp))
                            .padding(horizontal = 5.dp, vertical = 1.dp),
                    )
                }
            }
            if (c.snippet.isNotBlank()) {
                Text(
                    c.snippet,
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.testTag("row.snippet.${c.threadId}"),
                )
            }
            // A team conversation says who answers it, so two people do not both reply.
            TeamRow.assignee(c)?.let { who ->
                Text(
                    Texts.get(R.string.row_assignee, who),
                    color = theme.fgMuted,
                    fontSize = 11.sp,
                    maxLines = 1,
                    modifier = Modifier.testTag("row.assignee.${c.threadId}"),
                )
            }
        }
    }
}
