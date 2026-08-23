package jp.golia.mailrs.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.OutlinedTextField
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
import jp.golia.mailrs.accounts.AccountSender
import jp.golia.mailrs.accounts.AccountStore
import jp.golia.mailrs.accounts.MailAccount
import jp.golia.mailrs.accounts.OutgoingMessage
import kotlinx.coroutines.launch

/**
 * Writing a message from a connected mailbox.
 *
 * The From row is a picker even with one account, because which address
 * a message leaves by is the thing people get wrong and the thing they
 * cannot see afterwards. It is at the top, where every mail client puts
 * it.
 */
@Composable
fun ComposeMailScreen(
    accounts: List<MailAccount>,
    initial: OutgoingMessage.Draft,
    initialAccountId: String,
    onClose: () -> Unit,
) {
    val theme = LocalTheme.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val store = remember { AccountStore(context) }

    var from by remember {
        mutableStateOf(
            accounts.firstOrNull { it.id == initialAccountId } ?: accounts.firstOrNull(),
        )
    }
    var to by remember { mutableStateOf(initial.to.joinToString(", ")) }
    var subject by remember { mutableStateOf(initial.subject) }
    var body by remember { mutableStateOf(initial.body) }
    var sending by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf("") }

    BackHandler { onClose() }

    fun send() {
        val account = from ?: return
        if (sending) return
        sending = true
        failure = ""
        scope.launch {
            val draft = initial.copy(
                from = account.address,
                fromName = account.displayName,
                to = to.split(",").map { it.trim() }.filter { it.isNotEmpty() },
                subject = subject,
                body = body,
            )
            when (val outcome = AccountSender.send(draft, account, store)) {
                is AccountSender.Outcome.Sent -> onClose()
                is AccountSender.Outcome.Failed -> {
                    failure = outcome.why
                    sending = false
                }
            }
        }
    }

    Column(
        Modifier.fillMaxSize().background(theme.bg).verticalScroll(rememberScrollState()),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(onClick = onClose, modifier = Modifier.testTag("compose.cancel")) {
                Text("Cancel", color = theme.accent, fontSize = 13.sp)
            }
            Box(Modifier.weight(1f))
            when {
                sending -> CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else -> TextButton(
                    onClick = { send() },
                    modifier = Modifier.testTag("compose.send"),
                ) {
                    Text("Send", color = theme.accent, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }

        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("From", color = theme.fgMuted, fontSize = 11.sp)
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                for (account in accounts) {
                    FromChip(account, account.id == from?.id) { from = account }
                }
            }

            OutlinedTextField(
                value = to,
                onValueChange = { to = it },
                label = { Text("To") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("compose.to"),
            )
            OutlinedTextField(
                value = subject,
                onValueChange = { subject = it },
                label = { Text("Subject") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("compose.subject"),
            )
            OutlinedTextField(
                value = body,
                onValueChange = { body = it },
                label = { Text("Message") },
                modifier = Modifier.fillMaxWidth().heightIn(min = 200.dp).testTag("compose.body"),
            )

            if (failure.isNotEmpty()) {
                // In the screen, not a dialog that has to be dismissed
                // before the message can be fixed — what went wrong and
                // what to change are the same screen.
                Text(
                    failure,
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                    modifier = Modifier.testTag("compose.failure"),
                )
            }
        }
    }
}

@Composable
private fun FromChip(account: MailAccount, on: Boolean, onTap: () -> Unit) {
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
            .testTag("compose.from.${account.address}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(
            Modifier.size(8.dp).clip(CircleShape)
                .background(
                    Color(android.graphics.Color.parseColor(AccountColour.forId(account.id))),
                ),
        )
        Text(account.address, color = theme.fg, fontSize = 12.sp)
    }
}
