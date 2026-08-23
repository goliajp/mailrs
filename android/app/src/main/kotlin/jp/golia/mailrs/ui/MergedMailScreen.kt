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
import jp.golia.mailrs.accounts.MailboxMerge
import jp.golia.mailrs.accounts.MailboxRow
import jp.golia.mailrs.accounts.MailboxSyncRunner
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

    val filter = when {
        only.isEmpty() -> null
        else -> only
    }
    val visible = MailboxMerge.newestFirst(MailboxMerge.onlyAccounts(rows, filter))

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

    Column(Modifier.fillMaxSize().background(theme.bg)) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Other mail", color = theme.fg, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            Box(Modifier.weight(1f))
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
            val text = when {
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
                MergedMailRow(row, accounts.firstOrNull { it.id == row.accountId })
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

@Composable
private fun MergedMailRow(row: MailboxRow, account: MailAccount?) {
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
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)
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
