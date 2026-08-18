package jp.golia.mailrs.ui

import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.remember
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.DeleteOutline
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
import jp.golia.mailrs.Draft
import jp.golia.mailrs.UiState
import jp.golia.mailrs.closeDrafts
import jp.golia.mailrs.discardDraft
import jp.golia.mailrs.editSavedDraft
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.Wire

/**
 * Messages that were started and not sent.
 *
 * They live on the server, so a draft begun on the phone is there on the
 * web and the other way round — which is the only reason a drafts list
 * is worth having rather than a local autosave.
 *
 * **A row names what it can.** A draft's subject is often the last thing
 * written and often blank, so the recipient carries the row when it is,
 * and the body's first line when neither is there. A list of
 * "(no subject)" tells a reader nothing about which one to open.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DraftsScreen(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val snackbars = remember { SnackbarHostState() }
    // Seven screens could set an error and none of them said one out
    // loud; the snackbar built for exactly that was wired to three
    // others. A refused cancel here looks identical to a successful
    // one — the row is already gone.
    FailureSnackbar(state, vm, snackbars, hasContent = state.drafts.isNotEmpty())

    Scaffold(
        containerColor = theme.bg,
        snackbarHost = { SnackbarHost(snackbars) },
        topBar = {
            TopAppBar(
                title = { Text("Drafts", fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = theme.bg,
                    titleContentColor = theme.fg,
                ),
                navigationIcon = {
                    IconButton(
                        onClick = { vm.closeDrafts() },
                        modifier = Modifier.testTag("button.closeDrafts"),
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
                state.busy && state.drafts.isEmpty() ->
                    CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)

                state.drafts.isEmpty() ->
                    Text(
                        "Nothing half-written.",
                        color = theme.fgMuted,
                        fontSize = 13.sp,
                        modifier = Modifier.align(Alignment.Center).padding(32.dp).testTag("drafts.empty"),
                    )

                else -> LazyColumn(Modifier.fillMaxSize().testTag("list.drafts")) {
                    itemsIndexed(state.drafts, key = { _, d -> d.id }) { index, d ->
                        DraftRow(d, onOpen = { vm.editSavedDraft(d) }, onDiscard = { vm.discardDraft(d) })
                        // Between rows only — see the conversation list.
                        if (index < state.drafts.lastIndex) {
                            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DraftRow(d: Wire.Draft, onOpen: () -> Unit, onDiscard: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onOpen)
            .padding(start = 16.dp, end = 4.dp, top = 12.dp, bottom = 12.dp)
            .testTag("row.draft"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                headline(d),
                color = theme.fg,
                fontSize = 14.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                RowDate.format(d.updatedAt),
                color = theme.fgMuted,
                fontSize = 11.sp,
            )
        }
        IconButton(onClick = onDiscard, modifier = Modifier.testTag("button.discardDraft")) {
            Icon(
                Icons.Filled.DeleteOutline,
                contentDescription = "Discard",
                tint = theme.fgMuted,
                modifier = Modifier.size(18.dp),
            )
        }
    }
}

/** Subject, else who it is to, else how it starts. */
internal fun headline(d: Wire.Draft): String {
    d.subject.trim().takeIf { it.isNotEmpty() }?.let { return it }
    d.to.trim().takeIf { it.isNotEmpty() }?.let { return "To $it" }
    val firstLine = d.body.lineSequence().firstOrNull { it.isNotBlank() }?.trim()
    return firstLine?.take(60) ?: "Empty draft"
}
