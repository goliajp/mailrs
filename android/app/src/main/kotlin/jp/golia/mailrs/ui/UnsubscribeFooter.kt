package jp.golia.mailrs.ui

import android.content.Intent
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.net.toUri
import jp.golia.mailrs.UiState
import jp.golia.mailrs.Unsubscribing
import jp.golia.mailrs.unsubscribe
import jp.golia.mailrs.reportFailure
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.UnsubscribeOffer
import jp.golia.mailrs.wire.Wire

/**
 * The way off a mailing list, under the message that came from one.
 *
 * At the foot of the card rather than a banner over it: 42.6% of real
 * mail carries `List-Unsubscribe`, so a banner would be a stripe over
 * nearly every other message, and the reader who wants out has already
 * finished reading. The sender's own link is usually in the same place
 * and unfindable at phone size — this is the same action, in a fixed
 * spot, at a legible size.
 *
 * **Only one-click is performed here, and the server performs it.** The
 * advertised URLs identify the subscriber, so fetching one from a phone
 * hands the sender the reader's address and network; a page is offered
 * as a link and opened deliberately, never on their behalf.
 *
 * **A failure is said out loud, with the link still there.** An
 * unsubscribe that failed and looks like one that worked is how people
 * end up tapping it every week for a year.
 */
@Composable
fun UnsubscribeFooter(threadId: String, m: Wire.Message, state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val context = LocalContext.current
    val offer = UnsubscribeOffer.of(m.unsubscribe)
    if (offer is UnsubscribeOffer.None) return

    val progress = state.unsubscribing[m.uid]

    Row(Modifier.padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        when (progress) {
            Unsubscribing.Working -> {
                CircularProgressIndicator(Modifier.size(14.dp), color = theme.accent, strokeWidth = 2.dp)
                Text(
                    "Unsubscribing…",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(start = 8.dp),
                )
            }

            Unsubscribing.Done -> {
                Icon(
                    Icons.Filled.CheckCircle,
                    contentDescription = null,
                    tint = theme.success,
                    modifier = Modifier.size(14.dp),
                )
                Text(
                    "Unsubscribed",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(start = 6.dp).testTag("unsubscribed"),
                )
            }

            else -> {
                TextButton(
                    onClick = {
                        when (offer) {
                            UnsubscribeOffer.OneClick -> vm.unsubscribe(threadId, m.uid)
                            is UnsubscribeOffer.OpenPage -> open(context, vm, offer.url)
                            is UnsubscribeOffer.SendMail -> open(context, vm, offer.mailto)
                            UnsubscribeOffer.None -> Unit
                        }
                    },
                    modifier = Modifier.testTag("button.unsubscribe"),
                ) {
                    Text(offer.label, color = theme.accent, fontSize = 12.sp)
                }
                if (progress == Unsubscribing.Failed) {
                    Text(
                        "That did not go through",
                        color = theme.warning,
                        fontSize = 12.sp,
                        modifier = Modifier.testTag("unsubscribe.failed"),
                    )
                }
            }
        }
    }
}

/**
 * Hand the link to whatever opens it, and say when nothing does.
 *
 * A phone with no browser is unusual; a `mailto:` with no handler is
 * not, since this app is often the one that would have handled it and
 * excludes itself here. Either way a tap that silently does nothing
 * reads as a broken button.
 */
private fun open(context: android.content.Context, vm: MailViewModel, uri: String) {
    val opened = runCatching {
        context.startActivity(
            Intent(Intent.ACTION_VIEW, uri.toUri()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }.isSuccess
    if (!opened) vm.reportFailure("Nothing on this phone can open that link.")
}
