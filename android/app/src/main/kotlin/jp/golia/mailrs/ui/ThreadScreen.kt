package jp.golia.mailrs.ui

import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Forward
import androidx.compose.material.icons.automirrored.filled.Reply
import androidx.compose.material.icons.automirrored.filled.ReplyAll
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Code
import androidx.compose.material.icons.filled.MarkEmailRead
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.Draft
import jp.golia.mailrs.UiState
import jp.golia.mailrs.attachmentOpened
import jp.golia.mailrs.compose
import jp.golia.mailrs.draftNoticeShown
import jp.golia.mailrs.openDrafts
import jp.golia.mailrs.applyToSelection
import jp.golia.mailrs.clearSelection
import jp.golia.mailrs.dismissUndo
import jp.golia.mailrs.toggleSelected
import jp.golia.mailrs.triage
import jp.golia.mailrs.undo
import androidx.compose.material.icons.filled.MarkEmailUnread
import androidx.compose.material.icons.filled.StarBorder
import jp.golia.mailrs.toggleStar
import jp.golia.mailrs.triageOpenThread
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.SenderIdentity
import jp.golia.mailrs.wire.Wire

/** A thread: messages as cards on the grouped background. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ThreadScreen(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val open = state.open ?: return
    val context = LocalContext.current

    // Handing the file on is an action, so it happens once per file and
    // then the offer is cleared. Leaving it in state would re-open the
    // same attachment on every recomposition.
    LaunchedEffect(state.openFile) {
        val ready = state.openFile ?: return@LaunchedEffect
        val uri = androidx.core.content.FileProvider.getUriForFile(
            context,
            context.packageName + ".files",
            ready.file,
        )
        val intent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, ready.mimeType)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        // **And say so when nothing can open it.** The comment here used
        // to promise this and the code did not: `runCatching` swallowed
        // the ActivityNotFoundException and the tap did nothing at all,
        // which reads as a broken button rather than as a phone with no
        // PDF reader on it.
        val opened = runCatching { context.startActivity(intent) }.isSuccess
        vm.attachmentOpened()
        if (!opened) vm.reportFailure("No app on this phone opens ${ready.filename}.")
    }
    val snackbars = remember { SnackbarHostState() }
    FailureSnackbar(state, vm, snackbars, hasContent = state.messages.isNotEmpty())

    Scaffold(
        containerColor = theme.bgSecondary,
        snackbarHost = { SnackbarHost(snackbars) },
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        open.subject.ifBlank { "(no subject)" },
                        fontSize = 16.sp,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = theme.bgSecondary,
                    titleContentColor = theme.fg,
                ),
                navigationIcon = {
                    IconButton(onClick = { vm.closeThread() }, modifier = Modifier.testTag("button.back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back", tint = theme.fgSecondary)
                    }
                },
                // **Filing without going back first.** Reading a message
                // and wanting it out of the way is the commonest thing
                // that happens next, and it used to mean back, find the
                // row again, swipe. Archive uses the same deferred
                // triage a swipe does, so the undo snackbar on the list
                // offers it back.
                actions = {
                    IconButton(
                        onClick = { vm.toggleStar(open) },
                        modifier = Modifier.testTag("button.star"),
                    ) {
                        Icon(
                            if (open.flagged) Icons.Filled.Star else Icons.Filled.StarBorder,
                            contentDescription = if (open.flagged) "Unstar" else "Star",
                            tint = if (open.flagged) theme.warning else theme.fgSecondary,
                        )
                    }
                    IconButton(
                        onClick = { vm.triageOpenThread(MailrsClient.Verb.Unread) },
                        modifier = Modifier.testTag("button.markUnread"),
                    ) {
                        Icon(
                            Icons.Filled.MarkEmailUnread,
                            contentDescription = "Mark unread",
                            tint = theme.fgSecondary,
                        )
                    }
                    IconButton(
                        onClick = { vm.triageOpenThread(MailrsClient.Verb.Archive) },
                        modifier = Modifier.testTag("button.archive"),
                    ) {
                        Icon(Icons.Filled.Archive, contentDescription = "Archive", tint = theme.fgSecondary)
                    }
                },
            )
        }
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            when {
                state.busy && state.messages.isEmpty() ->
                    CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)
                state.error != null && state.messages.isEmpty() ->
                    Conclusion("Could not open this conversation", state.error, Modifier.align(Alignment.Center))
                else -> Column(
                    Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(12.dp)
                        .testTag("list.messages"),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    state.messages.forEach { MessageCard(open.threadId, it, state, vm) }
                }
            }
        }
    }
}

@Composable
private fun MessageCard(threadId: String, m: Wire.Message, state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(theme.surface)
            .padding(14.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SenderAvatarView(m.sender, size = 30.dp)
            Column(Modifier.padding(start = 10.dp).weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        SenderIdentity.readableName(m.sender),
                        color = theme.fg,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    SenderTrustMark(m.senderTrust)
                    SenderClaimMark(m.sender)
                }
                Text(RowDate.format(m.internalDate), color = theme.fgMuted, fontSize = 11.sp)
            }
        }
        // HTML first, and not as a preference. A message that was
        // composed as HTML and shown as its `text_body` is a different
        // message: the fixture's plain part is the two words "plain
        // fallback" against a newsletter, which is what this client
        // showed until now.
        val html = m.htmlBody
        if (html.isNullOrBlank()) {
            Text(
                m.textBody.orEmpty().ifBlank { "(no body)" },
                color = theme.fg,
                fontSize = 15.sp,
                modifier = Modifier.padding(top = 10.dp),
            )
        } else {
            MessageBody(html, Modifier.padding(top = 10.dp))
        }
        AttachmentList(m.uid, m.attachments, state, vm)
        UnsubscribeFooter(threadId, m, state, vm)

        Row(Modifier.padding(top = 6.dp)) {
            IconButton(
                onClick = { vm.compose(replyTo = m) },
                modifier = Modifier.testTag("button.reply"),
            ) {
                Icon(Icons.AutoMirrored.Filled.Reply, contentDescription = "Reply", tint = theme.fgSecondary)
            }
            IconButton(
                onClick = { vm.compose(replyTo = m, all = true) },
                modifier = Modifier.testTag("button.replyAll"),
            ) {
                Icon(Icons.AutoMirrored.Filled.ReplyAll, contentDescription = "Reply all", tint = theme.fgSecondary)
            }
            IconButton(
                onClick = { vm.compose(replyTo = m, forward = true) },
                modifier = Modifier.testTag("button.forward"),
            ) {
                Icon(
                    Icons.AutoMirrored.Filled.Forward,
                    contentDescription = "Forward",
                    tint = theme.fgSecondary,
                )
            }
            IconButton(
                onClick = { vm.viewSource(m.uid) },
                modifier = Modifier.testTag("button.viewSource"),
            ) {
                Icon(Icons.Filled.Code, contentDescription = "View source", tint = theme.fgSecondary)
            }
        }
    }
}

/**
 * State is a mark and a colour, not a sentence.
 *
 * `ios/DESIGN.md`: *"'Suspicious sender' spelled out beside a name and a
 * time was two words too many for the line; it is an orange shield now,
 * with the words kept as the accessibility label"*. The first Android
 * draft spelled it out, which is the thing that made the header wrap.
 *
 * **There is no positive mark** — see `SenderIdentity`.
 */
@Composable
private fun SenderTrustMark(senderTrust: String) {
    val theme = LocalTheme.current
    if (!SenderIdentity.isSuspicious(senderTrust)) return
    Icon(
        Icons.Filled.Warning,
        contentDescription = "Suspicious sender",
        tint = theme.warning,
        modifier = Modifier.padding(start = 5.dp).size(14.dp),
    )
}

/** Where it actually came from, when the name says somewhere else. */
@Composable
private fun SenderClaimMark(sender: String) {
    val theme = LocalTheme.current
    val actual = SenderIdentity.contradictedDomain(sender) ?: return
    Text(
        actual,
        color = theme.warning,
        fontSize = 10.sp,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier
            .padding(start = 5.dp)
            .background(theme.warning.copy(alpha = 0.14f), RoundedCornerShape(5.dp))
            .padding(horizontal = 5.dp, vertical = 1.dp),
    )
}

/** An absence, with the sentence finished. */
@Composable
internal fun Conclusion(headline: String, detail: String, modifier: Modifier = Modifier) {
    val theme = LocalTheme.current
    Column(
        modifier.padding(32.dp).testTag("conclusion"),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(headline, color = theme.fg, fontSize = 16.sp, fontWeight = FontWeight.Medium)
        Text(
            detail,
            color = theme.fgMuted,
            fontSize = 13.sp,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 6.dp),
        )
    }
}
