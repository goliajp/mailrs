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
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.SenderIdentity
import jp.golia.mailrs.wire.Wire

/**
 * The sign-in screen is a front door.
 *
 * `ios/DESIGN.md`: the mark, the name, one line of what this is, and a
 * full-width prominent button — *"not another table row"*. The first
 * Android draft was three bare fields and a text button, which is the
 * table row.
 *
 * Typing belongs at the top: the fields sit in the upper half so the
 * keyboard does not cover the thing the reader came to type into.
 */
@Composable
fun SignInScreen(busy: Boolean, error: String?, onSignIn: (String, String, String) -> Unit) {
    val theme = LocalTheme.current
    var server by remember { mutableStateOf("mail.golia.jp") }
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }

    Column(
        Modifier
            .fillMaxSize()
            .background(theme.bg)
            .padding(horizontal = 24.dp)
            .padding(top = 72.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        AppMark(size = 64.dp)
        Text(
            "Mailrs",
            color = theme.fg,
            fontSize = 30.sp,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.padding(top = 14.dp),
        )
        Text(
            "Your own mail server.",
            color = theme.fgMuted,
            fontSize = 14.sp,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 4.dp, bottom = 28.dp),
        )

        OutlinedTextField(
            value = server,
            onValueChange = { server = it },
            label = { Text("Server") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            modifier = Modifier.fillMaxWidth().testTag("field.server"),
        )
        OutlinedTextField(
            value = username,
            onValueChange = { username = it },
            label = { Text("Address") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
            modifier = Modifier.fillMaxWidth().padding(top = 10.dp).testTag("field.address"),
        )
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text("Password") },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            modifier = Modifier.fillMaxWidth().padding(top = 10.dp).testTag("field.password"),
        )

        if (error != null) {
            Text(
                error,
                color = theme.danger,
                fontSize = 13.sp,
                modifier = Modifier.fillMaxWidth().padding(top = 10.dp).testTag("text.signInError"),
            )
        }

        Button(
            onClick = { onSignIn(server, username, password) },
            enabled = !busy && username.isNotBlank() && password.isNotBlank(),
            colors = ButtonDefaults.buttonColors(containerColor = theme.accent, contentColor = theme.accentFg),
            shape = RoundedCornerShape(10.dp),
            modifier = Modifier.fillMaxWidth().padding(top = 20.dp).height(48.dp).testTag("button.signIn"),
        ) {
            Text(if (busy) "Signing in…" else "Sign in", fontWeight = FontWeight.SemiBold)
        }
    }
}

/**
 * The list.
 *
 * The title is inline, not large: *"a large title spends about fifty
 * points of every screen restating a word the toolbar has room for, and
 * this list is measured in rows"*.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConversationListScreen(state: MailViewModel.UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val snackbars = remember { SnackbarHostState() }
    var searchOpen by remember { mutableStateOf(false) }
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val selecting = state.selected.isNotEmpty()

    // A selection is a mode, and back leaves a mode before it leaves
    // the app. Without this, the gesture that everyone uses to say
    // "never mind" closed Mailrs with the rows still picked.
    BackHandler(enabled = selecting) { vm.clearSelection() }
    val scope = rememberCoroutineScope()

    // Collapsing the bar ends the search. Leaving a stale result set
    // behind the closed bar means re-opening it shows an answer to a
    // question the person has stopped asking.
    if (searchOpen) {
        MailSearchBar(
            state = state,
            vm = vm,
            expanded = true,
            onExpandedChange = { open ->
                searchOpen = open
                if (!open) vm.clearSearch()
            },
        )
        return
    }

    // The undo snackbar. `SnackbarResult` is what makes this the
    // platform's control and not a lookalike: the action, the timeout
    // and the swipe-away all come back through one return value, so
    // "the person did nothing" and "the person undid it" cannot be
    // confused — which is the whole safety of an undo.
    // "Draft saved", once. Said where the composer went, because a
    // message that vanished from the screen without a word looks lost.
    LaunchedEffect(state.draftSaved) {
        if (!state.draftSaved) return@LaunchedEffect
        snackbars.showSnackbar("Draft saved")
        vm.draftNoticeShown()
    }

    LaunchedEffect(state.undo) {
        val pending = state.undo ?: return@LaunchedEffect
        val what = when (pending.verb) {
            MailrsClient.Verb.Archive -> "Archived"
            MailrsClient.Verb.Read -> "Marked read"
            MailrsClient.Verb.Unarchive -> "Moved to inbox"
            MailrsClient.Verb.Unread -> "Marked unread"
            MailrsClient.Verb.Star -> "Starred"
            MailrsClient.Verb.Unstar -> "Unstarred"
            MailrsClient.Verb.Delete -> "Deleted"
        }
        val result = snackbars.showSnackbar(
            message = what,
            actionLabel = "Undo",
            duration = androidx.compose.material3.SnackbarDuration.Short,
        )
        if (result == SnackbarResult.ActionPerformed) vm.undo() else vm.dismissUndo()
    }

    ModalNavigationDrawer(
        drawerState = drawer,
        drawerContent = {
            MailListDrawer(
                current = state.list,
                onDrafts = {
                    scope.launch { drawer.close() }
                    vm.openDrafts()
                },
                onSettings = {
                    scope.launch { drawer.close() }
                    vm.openSettings()
                },
            ) { chosen ->
                scope.launch { drawer.close() }
                vm.show(chosen)
            }
        },
    ) {
    Scaffold(
        containerColor = theme.bg,
        snackbarHost = { SnackbarHost(snackbars, Modifier.testTag("snackbar.undo")) },
        topBar = {
            TopAppBar(
                // **The bar becomes the action bar.** Android's answer
                // to acting on many rows is that the top bar changes
                // rather than a second one appearing: a count on the
                // left where the title was, the actions on the right,
                // and a close where the drawer button was.
                title = {
                    if (selecting) {
                        Text("${state.selected.size}", fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                    } else {
                        Text(state.list.title, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                    }
                },
                navigationIcon = {
                    IconButton(
                        onClick = {
                            if (selecting) vm.clearSelection() else scope.launch { drawer.open() }
                        },
                        modifier = Modifier.testTag(if (selecting) "button.endSelection" else "button.folders"),
                    ) {
                        if (selecting) {
                            Icon(Icons.Filled.Close, contentDescription = "Done", tint = theme.fgSecondary)
                        } else {
                            Icon(Icons.Filled.Menu, contentDescription = "Lists", tint = theme.fgSecondary)
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = if (selecting) theme.bgTertiary else theme.bg,
                    titleContentColor = theme.fg,
                ),
                actions = {
                    if (selecting) {
                        IconButton(
                            onClick = { vm.applyToSelection(MailrsClient.Verb.Archive) },
                            modifier = Modifier.testTag("button.selectionArchive"),
                        ) {
                            Icon(Icons.Filled.Archive, contentDescription = "Archive", tint = theme.fgSecondary)
                        }
                        IconButton(
                            onClick = { vm.applyToSelection(MailrsClient.Verb.Read) },
                            modifier = Modifier.testTag("button.selectionRead"),
                        ) {
                            Icon(
                                Icons.Filled.MarkEmailRead,
                                contentDescription = "Mark read",
                                tint = theme.fgSecondary,
                            )
                        }
                        IconButton(
                            onClick = { vm.applyToSelection(MailrsClient.Verb.Star) },
                            modifier = Modifier.testTag("button.selectionStar"),
                        ) {
                            Icon(Icons.Filled.Star, contentDescription = "Star", tint = theme.fgSecondary)
                        }
                    } else {
                        IconButton(
                            onClick = { searchOpen = true },
                            modifier = Modifier.testTag("button.search"),
                        ) {
                            Icon(Icons.Filled.Search, contentDescription = "Search", tint = theme.fgSecondary)
                        }
                        IconButton(onClick = { vm.refresh() }, modifier = Modifier.testTag("button.refresh")) {
                            Icon(Icons.Filled.Refresh, contentDescription = "Refresh", tint = theme.fgSecondary)
                        }
                    }
                },
            )
        },
        floatingActionButton = {
            FloatingActionButton(
                onClick = { vm.compose() },
                containerColor = theme.accent,
                contentColor = theme.accentFg,
                modifier = Modifier.testTag("button.compose"),
            ) {
                Icon(Icons.Filled.Edit, contentDescription = "New message")
            }
        },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            // loading → failed → empty → content, in that order. "No
            // mail" printed while the request is still out is a claim
            // nobody made.
            when {
                state.busy && state.conversations.isEmpty() ->
                    CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)

                state.error != null && state.conversations.isEmpty() ->
                    Conclusion("Could not load your mail", state.error, Modifier.align(Alignment.Center))

                state.conversations.isEmpty() ->
                    // An empty state is a conclusion, so it finishes the
                    // sentence — and each list finishes its own. "All
                    // caught up" congratulates a reader on an empty spam
                    // folder and reads as loss in Archived.
                    Conclusion(
                        state.list.emptyMessage,
                        "Mail that arrives here appears in this list.",
                        Modifier.align(Alignment.Center),
                    )

                else -> PullToRefreshBox(
                    isRefreshing = state.busy,
                    onRefresh = { vm.refresh() },
                    modifier = Modifier.fillMaxSize(),
                ) {
                    LazyColumn(Modifier.fillMaxSize().testTag("list.conversations")) {
                        items(state.conversations, key = { it.threadId }) { c ->
                            val picked = c.threadId in state.selected
                            if (selecting) {
                                // No swipe while selecting: the gesture
                                // that picks rows and the gesture that
                                // files them cannot share a finger.
                                ConversationRow(
                                    c,
                                    selected = picked,
                                    onLongPress = { vm.toggleSelected(c.threadId) },
                                ) { vm.toggleSelected(c.threadId) }
                            } else {
                                SwipeableConversationRow(
                                    onArchive = { vm.triage(c, MailrsClient.Verb.Archive) },
                                    onMarkRead = { vm.triage(c, MailrsClient.Verb.Read) },
                                ) {
                                    ConversationRow(
                                        c,
                                        selected = false,
                                        onLongPress = { vm.toggleSelected(c.threadId) },
                                    ) { vm.open(c) }
                                }
                            }
                            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                        }
                    }
                }
            }
        }
    }
}
}

/** A thread: messages as cards on the grouped background. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ThreadScreen(state: MailViewModel.UiState, vm: MailViewModel) {
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
        // No app on the phone opens this kind of file. Saying so beats
        // a tap that appears to do nothing.
        runCatching { context.startActivity(intent) }
        vm.attachmentOpened()
    }
    Scaffold(
        containerColor = theme.bgSecondary,
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
private fun MessageCard(threadId: String, m: Wire.Message, state: MailViewModel.UiState, vm: MailViewModel) {
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
private fun Conclusion(headline: String, detail: String, modifier: Modifier = Modifier) {
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
