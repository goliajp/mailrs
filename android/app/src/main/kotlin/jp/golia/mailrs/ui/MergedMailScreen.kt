package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.accounts.AccountColour
import jp.golia.mailrs.accounts.AccountStore
import jp.golia.mailrs.accounts.MailAccount
import jp.golia.mailrs.accounts.MailboxActions
import jp.golia.mailrs.accounts.MailboxMerge
import jp.golia.mailrs.accounts.MailboxRow
import jp.golia.mailrs.accounts.MailboxSearch
import jp.golia.mailrs.accounts.MailboxSyncRunner
import jp.golia.mailrs.accounts.MessageReader
import jp.golia.mailrs.accounts.OutgoingMessage
import jp.golia.mailrs.accounts.ReplyDraft
import kotlinx.coroutines.launch

/**
 * One list for every connected mailbox.
 *
 * The shape every working mail client settles on: one list by default,
 * and a way to narrow it. Narrowing is a row of chips rather than a
 * menu, because the useful question — "is anything in here from work?"
 * — is answered by looking, not by opening something.
 */
@Composable
fun MergedMailScreen() {
    val theme = LocalTheme.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val store = remember { AccountStore(context) }

    var accounts by remember { mutableStateOf(store.load()) }
    var rows by remember { mutableStateOf(store.rows()) }
    // Empty means **no filter** — the ordinary case, and the one a
    // person gets without choosing anything.
    var only by remember { mutableStateOf(emptySet<String>()) }
    var syncing by remember { mutableStateOf(false) }
    var failures by remember { mutableStateOf(emptyMap<String, String>()) }
    /** The message being read, if any. */
    var opened by remember { mutableStateOf<MailboxRow?>(null) }
    var query by remember { mutableStateOf("") }
    /** The message being written, if any, and which account it leaves by. */
    var writing by remember { mutableStateOf<Pair<OutgoingMessage.Draft, String>?>(null) }

    val filter = when {
        only.isEmpty() -> null
        else -> only
    }
    val visible = MailboxSearch.matches(
        MailboxMerge.newestFirst(MailboxMerge.onlyAccounts(rows, filter)),
        query,
    )

    fun sync() {
        if (syncing) return
        syncing = true
        failures = emptyMap()
        scope.launch {
            val targets = when {
                only.isEmpty() -> accounts
                else -> accounts.filter { it.id in only }
            }
            // Sequential rather than parallel: a phone on a train
            // opening six TLS connections at once finishes no sooner
            // and fails messier, and each account's rows land as it
            // goes so the list fills in rather than waiting for the
            // slowest server.
            for (account in targets) {
                val outcome = MailboxSyncRunner.run(account, store)
                if (outcome.failure != null) {
                    failures = failures + (account.id to outcome.failure)
                }
                rows = store.rows()
            }
            syncing = false
        }
    }

    writing?.let { (draft, accountId) ->
        ComposeMailScreen(accounts, draft, accountId) { writing = null }
        return
    }

    opened?.let { row ->
        val account = accounts.firstOrNull { it.id == row.accountId }
        MessageScreen(
            row,
            account,
            onReply = { loaded, all ->
                // The reply leaves by the account the message arrived
                // at. Replying from a different address than the one
                // that was written to is a mistake nobody notices until
                // the answer goes missing.
                account?.let {
                    writing = ReplyDraft.make(loaded.headers, it, loaded.text, all) to it.id
                    opened = null
                }
            },
        ) {
            opened = null
            // Reading marks a message read on the server and on this
            // device; the list has to be told, or it goes on showing it
            // as unread until the next fetch.
            rows = store.rows()
        }
        return
    }

    Column(Modifier.fillMaxSize().background(theme.bg)) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Other mail", color = theme.fg, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            Box(Modifier.weight(1f))
            // Writing does not depend on having fetched anything, so it
            // is offered as soon as there is an account to send from.
            if (accounts.isNotEmpty()) {
                TextButton(
                    onClick = {
                        val account = accounts.firstOrNull { it.id in only } ?: accounts.first()
                        writing = OutgoingMessage.Draft(
                            from = account.address,
                            fromName = account.displayName,
                            to = emptyList(),
                        ) to account.id
                    },
                    modifier = Modifier.testTag("mail.compose"),
                ) {
                    Text("New", color = theme.accent, fontSize = 13.sp)
                }
            }
            when {
                syncing -> CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else -> TextButton(onClick = { sync() }, modifier = Modifier.testTag("mail.sync")) {
                    Text("Fetch", color = theme.accent, fontSize = 13.sp)
                }
            }
        }

        if (accounts.isEmpty()) {
            Text(
                "No mailboxes yet. Add one in Settings to read it here.",
                color = theme.fgMuted,
                fontSize = 13.sp,
                modifier = Modifier.padding(16.dp).testTag("mail.empty"),
            )
            return@Column
        }

        androidx.compose.material3.OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            label = { Text("Search") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 4.dp)
                .testTag("mail.search"),
        )

        if (accounts.size > 1) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                for (account in accounts) {
                    AccountChip(account, account.id in only) {
                        only = when {
                            account.id in only -> only - account.id
                            else -> only + account.id
                        }
                    }
                }
            }
        }

        // An account that could not be read says so by name, in the
        // list rather than in a dialog: one unreachable server must not
        // stand in front of the mail from the five that answered.
        for (account in accounts) {
            val why = failures[account.id] ?: continue
            Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
                Text(account.title, color = theme.fg, fontSize = 12.sp)
                Text(why, color = theme.fgMuted, fontSize = 11.sp)
            }
        }

        if (visible.isEmpty()) {
            // Three different nothings, and they lead somewhere
            // different each: fetch, widen the filter, or know that a
            // local search cannot see what was never fetched.
            val text = when {
                query.isNotEmpty() ->
                    "Nothing here matches. Only mail already fetched is searched."
                only.isEmpty() -> "No mail yet. Fetch to read it."
                else -> "Nothing from the mailboxes you picked."
            }
            Text(
                text,
                color = theme.fgMuted,
                fontSize = 13.sp,
                modifier = Modifier.padding(16.dp).testTag("mail.nothing"),
            )
        }

        LazyColumn(Modifier.fillMaxSize()) {
            items(visible, key = { it.id }) { row ->
                val account = accounts.firstOrNull { it.id == row.accountId }
                MergedMailRow(
                    row,
                    account,
                    onTap = { opened = row },
                    onDelete = {
                        account?.let {
                            scope.launch {
                                when (val out = MailboxActions.delete(it, row, store)) {
                                    is MailboxActions.Outcome.Done -> rows = store.rows()
                                    is MailboxActions.Outcome.Failed ->
                                        failures = failures + (it.id to out.why)
                                }
                            }
                        }
                    },
                    onMarkUnread = {
                        account?.let {
                            scope.launch {
                                MailboxActions.markUnread(it, row, store)
                                rows = store.rows()
                            }
                        }
                    },
                )
            }
        }
    }
}

@Composable
private fun AccountChip(account: MailAccount, on: Boolean, onTap: () -> Unit) {
    val theme = LocalTheme.current
    val background = when {
        on -> theme.accent.copy(alpha = 0.18f)
        else -> theme.bgSecondary
    }
    Row(
        Modifier
            .clip(RoundedCornerShape(14.dp))
            .background(background)
            .clickable { onTap() }
            .padding(horizontal = 10.dp, vertical = 6.dp)
            .testTag("mail.filter.${account.address}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(
            Modifier.size(8.dp).clip(CircleShape)
                .background(Color(android.graphics.Color.parseColor(AccountColour.forId(account.id)))),
        )
        Text(account.title, color = theme.fg, fontSize = 12.sp)
    }
}

/**
 * One message in the merged list.
 *
 * Swiping is the gesture every mail client uses, and the two directions
 * are chosen the way they are everywhere: **the destructive one is the
 * one nobody reaches for by accident**, and the reversible one is on
 * the side a thumb rests.
 */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun MergedMailRow(
    row: MailboxRow,
    account: MailAccount?,
    onTap: () -> Unit,
    onDelete: () -> Unit,
    onMarkUnread: () -> Unit,
) {
    val state = androidx.compose.material3.rememberSwipeToDismissBoxState()
    // Driven from the settled value rather than from a veto callback:
    // the callback overload is deprecated, and vetoing was the wrong
    // shape anyway — the row goes when the **server** says it has, and
    // one that vanishes before that comes back on the next fetch
    // looking like a bug. So the swipe springs back and the list
    // updates when the action lands.
    LaunchedEffect(state.currentValue) {
        when (state.currentValue) {
            androidx.compose.material3.SwipeToDismissBoxValue.EndToStart -> {
                onDelete()
                state.reset()
            }
            androidx.compose.material3.SwipeToDismissBoxValue.StartToEnd -> {
                onMarkUnread()
                state.reset()
            }
            else -> Unit
        }
    }
    androidx.compose.material3.SwipeToDismissBox(
        state = state,
        backgroundContent = { SwipeBackground(state.dismissDirection) },
        modifier = Modifier.testTag("mail.swipe.${row.id}"),
    ) {
        MergedMailRowBody(row, account, onTap)
    }
}

@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun SwipeBackground(direction: androidx.compose.material3.SwipeToDismissBoxValue) {
    val theme = LocalTheme.current
    val text = when (direction) {
        androidx.compose.material3.SwipeToDismissBoxValue.EndToStart -> "Delete"
        androidx.compose.material3.SwipeToDismissBoxValue.StartToEnd -> "Unread"
        else -> ""
    }
    val alignment = when (direction) {
        androidx.compose.material3.SwipeToDismissBoxValue.EndToStart -> Alignment.CenterEnd
        else -> Alignment.CenterStart
    }
    Box(
        Modifier.fillMaxWidth().background(theme.bgSecondary).padding(horizontal = 20.dp),
        contentAlignment = alignment,
    ) {
        Text(text, color = theme.fgMuted, fontSize = 12.sp)
    }
}

@Composable
private fun MergedMailRowBody(row: MailboxRow, account: MailAccount?, onTap: () -> Unit) {
    val theme = LocalTheme.current
    // Unread is heavier. The only thing on the row that says so without
    // colour, which is why it is weight and not a tint.
    val weight = when {
        row.seen -> FontWeight.Normal
        else -> FontWeight.SemiBold
    }
    val subjectColour = when {
        row.seen -> theme.fgMuted
        else -> theme.fg
    }
    Row(
        Modifier.fillMaxWidth().clickable { onTap() }
            .padding(horizontal = 16.dp, vertical = 8.dp)
            .testTag("mail.row.${row.id}"),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(
            Modifier.size(8.dp).clip(CircleShape)
                .background(Color(android.graphics.Color.parseColor(AccountColour.forId(row.accountId)))),
        )
        Column {
            Text(row.displaySender, color = theme.fg, fontSize = 14.sp, fontWeight = weight)
            Text(row.displaySubject, color = subjectColour, fontSize = 13.sp, maxLines = 2)
            // Which mailbox, in words. The dot is a shortcut for people
            // who can see it; this line is the answer for everybody else.
            Text(
                "${account?.title ?: "Unknown"} · ${row.folder}",
                color = theme.fgMuted,
                fontSize = 11.sp,
            )
        }
    }
}
