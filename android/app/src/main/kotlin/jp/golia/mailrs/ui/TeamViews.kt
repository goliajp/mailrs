package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.Draft
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.UiState
import jp.golia.mailrs.replyHint
import jp.golia.mailrs.teamNotes
import jp.golia.mailrs.wire.TeamWire
import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.wire.Wire

private val noteTint = Color(0xFFB45309)

/**
 * A team conversation's internal notes, under its messages. Read only on
 * the phone: notes are written on the web. Marked as the team's own, the
 * way the web marks them: a note is never part of anything sent.
 */
@Composable
fun TeamNotes(open: Wire.Conversation, vm: MailViewModel) {
    val c = TeamWire.Conv.of(open.sharedConv) ?: return
    val notes by produceState<List<TeamWire.Note>?>(null, c) { value = vm.teamNotes(c) }
    val theme = LocalTheme.current
    Column(
        Modifier
            .fillMaxWidth()
            .background(noteTint.copy(alpha = 0.06f), RoundedCornerShape(8.dp))
            .padding(10.dp)
            .testTag("team.notes"),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(
            Texts.get(R.string.team_notes_heading),
            color = noteTint,
            fontSize = 12.sp,
            fontWeight = FontWeight.Medium,
        )
        val shown = notes ?: return@Column
        if (shown.isEmpty()) {
            Text(Texts.get(R.string.team_notes_empty), color = theme.fgMuted, fontSize = 12.sp)
        }
        for (note in shown) {
            Column(Modifier.testTag("team.note.${note.id}")) {
                Text(note.text, color = theme.fg, fontSize = 14.sp)
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(TeamWire.localPart(note.by), color = theme.fgMuted, fontSize = 11.sp)
                    Text(RowDate.format(note.at), color = theme.fgMuted, fontSize = 11.sp)
                    if (note.editedAt != null) {
                        Text(Texts.get(R.string.team_notes_edited), color = theme.fgMuted, fontSize = 11.sp)
                    }
                }
            }
        }
    }
}

/** The hint for a reply to the conversation that is open, when it is a team one. */
@Composable
fun TeamReplyHint(state: UiState, draft: Draft, vm: MailViewModel) {
    val open = state.open ?: return
    if (draft.replyToThreadId != open.threadId) return
    ReplyClaimHint(open.sharedConv, vm)
}

/**
 * Above a reply to a team conversation: who else is on it, so two people
 * do not answer the same customer.
 */
@Composable
fun ReplyClaimHint(sharedConv: String, vm: MailViewModel) {
    val c = TeamWire.Conv.of(sharedConv) ?: return
    val hint by produceState<TeamWire.ReplyHint?>(null, c) { value = vm.replyHint(c) }
    val text = when (val h = hint) {
        is TeamWire.ReplyHint.Writing -> Texts.get(R.string.team_writing, h.name)
        is TeamWire.ReplyHint.Assigned -> Texts.get(R.string.row_assignee, h.name)
        null -> return
    }
    Text(
        text,
        color = noteTint,
        fontSize = 12.sp,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp).testTag("reply.claimHint"),
    )
}
