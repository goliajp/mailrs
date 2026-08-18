package jp.golia.mailrs.ui

import androidx.compose.foundation.clickable
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
                    items(state.sentMail, key = SendJoin.Row::key) { row ->
                        SentRow(row) { vm.openThreadById(row.threadId) }
                        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                    }
                }
            }
        }
    }
}

@Composable
private fun SentRow(row: SendJoin.Row, onOpen: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onOpen)
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
        }
    }
}

private fun statusColour(status: String, theme: Theme) = when (status) {
    "delivered", "sent" -> theme.success
    "failed", "bounced" -> theme.danger
    else -> theme.fgMuted
}
