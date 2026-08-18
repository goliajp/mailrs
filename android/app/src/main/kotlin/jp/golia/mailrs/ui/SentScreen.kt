package jp.golia.mailrs.ui

import androidx.compose.foundation.combinedClickable
import jp.golia.mailrs.viewSendSource
import jp.golia.mailrs.redraft
import jp.golia.mailrs.resend
import androidx.compose.material3.TextButton
import jp.golia.mailrs.wire.Wire
import jp.golia.mailrs.cancelScheduled
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.UiState
import jp.golia.mailrs.closeSent
import jp.golia.mailrs.openThreadById
import jp.golia.mailrs.wire.SendJoin

/**
 * What was sent, and whether it arrived.
 *
 * Not a folder — the sent axis and the delivery projection are separate
 * endpoints joined on Message-ID (`SendJoin`), which is why this is its
 * own screen rather than a seventh entry in the list drawer.
 *
 * **A row with no status says nothing rather than "delivered".** Most
 * mail older than the projection has none, and claiming delivery for
 * something nobody tracked is the one thing this screen must not do.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SentScreen(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    Scaffold(
        containerColor = theme.bg,
        topBar = {
            TopAppBar(
                title = { Text("Sent", fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = theme.bg,
                    titleContentColor = theme.fg,
                ),
                navigationIcon = {
                    IconButton(
                        onClick = { vm.closeSent() },
                        modifier = Modifier.testTag("button.closeSent"),
                    ) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                            tint = theme.fgSecondary,
                        )
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            when {
                state.busy && state.sentMail.isEmpty() ->
                    CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)

                state.sentMail.isEmpty() ->
                    Text(
                        "Nothing sent yet.",
                        color = theme.fgMuted,
                        fontSize = 13.sp,
                        modifier = Modifier.align(Alignment.Center).padding(32.dp).testTag("sent.empty"),
                    )

                else -> LazyColumn(Modifier.fillMaxSize().testTag("list.sent")) {
                    // Above what has already gone, because this is the
                    // half a person can still do something about.
                    if (state.scheduled.isNotEmpty()) {
                        item {
                            Text(
                                "Waiting to send",
                                color = theme.fgMuted,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Medium,
                                modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
                            )
                        }
                        items(state.scheduled, key = Wire.ScheduledSend::id) { waiting ->
                            ScheduledRow(waiting) { vm.cancelScheduled(waiting) }
                            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                        }
                    }
                    items(state.sentMail, key = SendJoin.Row::key) { row ->
                        SentRow(
                            row,
                            onOpen = { vm.openThreadById(row.threadId) },
                            onResend = { vm.resend(row) },
                            onRedraft = { vm.redraft(row) },
                            onSource = { vm.viewSendSource(row) },
                        )
                        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                    }
                }
            }
        }
    }
}

/**
 * A message that has not left yet.
 *
 * With the one thing worth doing to it. A phone that can schedule a
 * message and not un-schedule it is worse than one that cannot
 * schedule at all.
 */
@Composable
private fun ScheduledRow(send: Wire.ScheduledSend, onCancel: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp)
            .testTag("row.scheduled"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                send.subject.ifBlank { "(no subject)" },
                color = theme.fg,
                fontSize = 14.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                "To ${send.recipient} · ${RowDate.format(send.scheduledAt)}",
                color = theme.warning,
                fontSize = 11.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        TextButton(onClick = onCancel, modifier = Modifier.testTag("button.cancelScheduled")) {
            Text("Cancel", color = theme.accent, fontSize = 13.sp)
        }
    }
}

@Composable
private fun SentRow(
    row: SendJoin.Row,
    onOpen: () -> Unit,
    onResend: () -> Unit,
    onRedraft: () -> Unit,
    onSource: () -> Unit,
) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
            .combinedClickable(
                onClick = onOpen,
                // The bytes that actually left, on a long press — the
                // same gesture the conversation list uses for its own
                // second meaning, and not worth a permanent button on
                // a row that already carries two.
                onLongClick = onSource,
                onLongClickLabel = "View source",
            )
            .padding(horizontal = 16.dp, vertical = 12.dp)
            .testTag("row.sent"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                row.subject.ifBlank { "(no subject)" },
                color = theme.fg,
                fontSize = 14.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                "To ${row.to}",
                color = theme.fgMuted,
                fontSize = 11.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Column(horizontalAlignment = Alignment.End) {
            Text(RowDate.format(row.date), color = theme.fgMuted, fontSize = 11.sp)
            // Absent for anything the projection never saw, and absent
            // is what it says — no badge at all rather than a hopeful
            // one.
            row.status?.let {
                Text(
                    it,
                    color = statusColour(it, theme),
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Medium,
                    modifier = Modifier.testTag("text.sendStatus"),
                )
            }
            // Only where the server says the bytes are still there.
            // Offered against anything else it answers 409, and a
            // button that fails after the tap is worse than none.
            if (row.canResend) {
                Row {
                    // Edit first: a send that failed because the address
                    // was wrong fails again unchanged, and "Send again"
                    // sends the stored bytes exactly as they were.
                    TextButton(onClick = onRedraft, modifier = Modifier.testTag("button.redraft")) {
                        Text("Edit", color = theme.accent, fontSize = 12.sp)
                    }
                    TextButton(onClick = onResend, modifier = Modifier.testTag("button.resend")) {
                        Text("Send again", color = theme.accent, fontSize = 12.sp)
                    }
                }
            }
        }
    }
}

private fun statusColour(status: String, theme: Theme) = when (status) {
    "delivered", "sent" -> theme.success
    "failed", "bounced" -> theme.danger
    else -> theme.fgMuted
}
