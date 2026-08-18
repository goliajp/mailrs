package jp.golia.mailrs.ui

import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import jp.golia.mailrs.markAllRead
import jp.golia.mailrs.openSent
import androidx.compose.foundation.focusable
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MarkEmailRead
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.rememberDrawerState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.Draft
import jp.golia.mailrs.UiState
import jp.golia.mailrs.compose
import jp.golia.mailrs.draftNoticeShown
import jp.golia.mailrs.openDrafts
import jp.golia.mailrs.applyToSelection
import jp.golia.mailrs.clearSelection
import jp.golia.mailrs.dismissUndo
import jp.golia.mailrs.toggleSelected
import jp.golia.mailrs.triage
import jp.golia.mailrs.undo
import jp.golia.mailrs.openSettings
import jp.golia.mailrs.setSearchOpen
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.MailrsClient

/**
 * The list.
 *
 * The title is inline, not large: *"a large title spends about fifty
 * points of every screen restating a word the toolbar has room for, and
 * this list is measured in rows"*.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConversationListScreen(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val snackbars = remember { SnackbarHostState() }


    // The launcher's Search shortcut. Read here rather than called into
    // the screen, because the shortcut can arrive before this list is
    // composed and a flag cannot be missed by being early.
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
    if (state.searchOpen) {
        MailSearchBar(
            state = state,
            vm = vm,
            expanded = true,
            onExpandedChange = { open -> vm.setSearchOpen(open) },
        )
        return
    }

    // The undo snackbar. `SnackbarResult` is what makes this the
    // platform's control and not a lookalike: the action, the timeout
    // and the swipe-away all come back through one return value, so
    // "the person did nothing" and "the person undid it" cannot be
    // confused — which is the whole safety of an undo.
    FailureSnackbar(state, vm, snackbars, hasContent = state.conversations.isNotEmpty())

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
                onSent = {
                    scope.launch { drawer.close() }
                    vm.openSent()
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
                            onClick = { vm.setSearchOpen(true) },
                            modifier = Modifier.testTag("button.search"),
                        ) {
                            Icon(Icons.Filled.Search, contentDescription = "Search", tint = theme.fgSecondary)
                        }
                        IconButton(onClick = { vm.refresh() }, modifier = Modifier.testTag("button.refresh")) {
                            Icon(Icons.Filled.Refresh, contentDescription = "Refresh", tint = theme.fgSecondary)
                        }
                        // The overflow, which is where Android puts what
                        // is wanted occasionally — a permanent button
                        // for "mark all read" would sit next to Search
                        // and Refresh being pressed by accident.
                        var menuOpen by rememberSaveable { mutableStateOf(false) }
                        IconButton(
                            onClick = { menuOpen = true },
                            modifier = Modifier.testTag("button.listMenu"),
                        ) {
                            Icon(
                                Icons.Filled.MoreVert,
                                contentDescription = "More",
                                tint = theme.fgSecondary,
                            )
                        }
                        DropdownMenu(
                            expanded = menuOpen,
                            onDismissRequest = { menuOpen = false },
                            containerColor = theme.surface,
                        ) {
                            DropdownMenuItem(
                                text = { Text("Mark all read", color = theme.fg, fontSize = 14.sp) },
                                onClick = {
                                    menuOpen = false
                                    vm.markAllRead()
                                },
                                modifier = Modifier.testTag("menu.markAllRead"),
                            )
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
                    val rows = rememberLazyListState()
                    // **Ask for the next page before the bottom.** The
                    // mailbox is thousands of threads and a page is
                    // fifty; waiting for the very last row to be visible
                    // means the reader waits too. Five from the end is
                    // about a screen's worth of warning.
                    val nearTheEnd by remember(state.conversations.size) {
                        derivedStateOf {
                            val last = rows.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
                            last >= state.conversations.size - 5
                        }
                    }
                    LaunchedEffect(nearTheEnd, state.conversations.size) {
                        if (nearTheEnd) vm.loadMore()
                    }
                    // **A keyboard does the two things people do most.**
                    // This app has a two-pane layout because it runs on
                    // tablets and opened foldables, and those are the
                    // devices that arrive with a keyboard — where `c`
                    // and `/` are what every mail client answers. The
                    // list is focusable so it can receive them at all;
                    // a key that reaches nothing is why a keyboard on
                    // Android so often does nothing.
                    val listFocus = remember { FocusRequester() }
                    LaunchedEffect(Unit) { listFocus.requestFocus() }
                    LazyColumn(
                        state = rows,
                        modifier = Modifier
                            .fillMaxSize()
                            .focusRequester(listFocus)
                            .focusable()
                            .onPreviewKeyEvent { event ->
                                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                                when (event.key) {
                                    Key.C -> { vm.compose(); true }
                                    Key.Slash -> { vm.setSearchOpen(true); true }
                                    else -> false
                                }
                            }
                            .testTag("list.conversations"),
                    ) {
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
                                        onArchive = { vm.triage(c, MailrsClient.Verb.Archive) },
                                        onMarkRead = { vm.triage(c, MailrsClient.Verb.Read) },
                                    ) { vm.open(c) }
                                }
                            }
                            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                        }
                        if (state.loadingMore) {
                            item {
                                Box(
                                    Modifier.fillMaxWidth().padding(16.dp),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    CircularProgressIndicator(
                                        Modifier.size(20.dp),
                                        color = theme.accent,
                                        strokeWidth = 2.dp,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
}
