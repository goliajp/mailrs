package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.SenderIdentity
import jp.golia.mailrs.wire.ThreadCollapse
import jp.golia.mailrs.wire.Wire

/** A folded message: one line with who, when and the start of what it said; a tap opens it. */
@Composable
fun FoldedMessage(m: Wire.Message, onOpen: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(theme.surface)
            .clickable(onClick = onOpen)
            .padding(horizontal = 14.dp, vertical = 10.dp)
            .testTag("message.folded.${m.uid}"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SenderAvatarView(m.sender, size = 24.dp)
        Column(Modifier.padding(start = 10.dp).weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    SenderIdentity.readableName(m.sender),
                    color = theme.fg,
                    fontSize = 13.sp,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                Text(
                    RowDate.format(m.internalDate),
                    color = theme.fgMuted,
                    fontSize = 11.sp,
                    modifier = Modifier.padding(start = 8.dp),
                )
            }
            Text(
                ThreadCollapse.snippet(m.textBody),
                color = theme.fgSecondary,
                fontSize = 13.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** `uid` added when absent, taken out when present. */
fun Set<Int>.toggle(uid: Int): Set<Int> {
    if (uid in this) return this - uid
    return this + uid
}
