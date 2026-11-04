package jp.golia.mailrs

import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.TeamWire
import jp.golia.mailrs.wire.teamNotes
import jp.golia.mailrs.wire.teamOverview
import kotlinx.coroutines.flow.update

/**
 * A team conversation's notes and who is on it, read only. A failure is
 * said in the snackbar and leaves the part empty rather than guessed.
 */
suspend fun MailViewModel.teamNotes(c: TeamWire.Conv): List<TeamWire.Note>? =
    when (val r = client.teamNotes(c)) {
        is MailrsClient.Outcome.Ok -> r.value
        is MailrsClient.Outcome.Err -> {
            _state.update { it.copy(error = r.message) }
            null
        }
    }

suspend fun MailViewModel.replyHint(c: TeamWire.Conv): TeamWire.ReplyHint? =
    when (val r = client.teamOverview(c)) {
        is MailrsClient.Outcome.Ok -> TeamWire.replyHint(r.value, _state.value.role?.address.orEmpty())
        is MailrsClient.Outcome.Err -> {
            _state.update { it.copy(error = r.message) }
            null
        }
    }
