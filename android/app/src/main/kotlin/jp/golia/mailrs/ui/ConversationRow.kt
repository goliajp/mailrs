package jp.golia.mailrs.ui

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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.SenderIdentity
import jp.golia.mailrs.wire.Wire

/**
 * One conversation, as a row — the unit everything multiplies.
 *
 * Two lines, no preview. `ios/DESIGN.md`: *"the preview line answers a
 * question triage doesn't ask, and costs a third of every row"*, a call
 * the web made on 2026-07-17 and iOS inherited. The first Android draft
 * had a snippet line, which is exactly the thing all three had already
 * decided against.
 *
 *     ● sender·······················×3  14:32
 *       subject····························★
 *
 * - Line 1: sender, semibold when unread; `×N` only when N > 1; date.
 * - Line 2: subject, secondary; star when flagged.
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
    val sender = c.participants.firstOrNull().orEmpty()

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
                    onArchive?.let { CustomAccessibilityAction("Archive") { it(); true } },
                    onMarkRead?.let { CustomAccessibilityAction("Mark read") { it(); true } },
                )
            }
            .padding(horizontal = 16.dp, vertical = 6.dp)
            .alpha(if (unread) 1f else 0.7f),
        verticalAlignment = Alignment.Top,
    ) {
        SenderAvatarView(sender, unread = unread)

        Column(Modifier.padding(start = 12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = SenderIdentity.readableName(sender).ifBlank { "(no sender)" },
                    color = theme.fg,
                    fontSize = 15.sp,
                    fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Normal,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
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
                        contentDescription = if (c.importanceLevel == "critical") "Critical" else "Important",
                        tint = if (c.importanceLevel == "critical") theme.danger else theme.warning,
                        modifier = Modifier.size(13.dp),
                    )
                }
                Text(
                    text = c.subject.ifBlank { "(no subject)" },
                    color = theme.fgSecondary,
                    fontSize = 14.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                if (c.flagged) {
                    Icon(
                        Icons.Filled.Star,
                        contentDescription = "Flagged",
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
        }
    }
}
