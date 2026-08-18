package jp.golia.mailrs.ui

import androidx.compose.runtime.remember
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.AccountDetail
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import jp.golia.mailrs.closeAccount
import jp.golia.mailrs.setAccountQuota
import jp.golia.mailrs.wire.QuotaInput
import jp.golia.mailrs.MailViewModel

/**
 * One account, opened.
 *
 * The three things about an account kept somewhere other than its row:
 * how much it may hold, the sieve script that files its mail, and what
 * is subscribed to its events.
 *
 * **The quota can be changed here; the other two cannot.** A sieve
 * script is a program and a phone keyboard is the wrong place to edit
 * one, and a webhook subscription belongs to whatever created it.
 * A quota is one number, and the argument for withholding it — that it
 * deserves a screen with room to say what it costs — did not survive
 * the question of what that room would say: the cost is the number.
 * Showing the other two is still the useful half — an operator asking
 * "why did that message go to Ops" wants to read the rule, not
 * rewrite it.
 *
 * **The signing secret is never shown.** It is what proves a delivery
 * came from this server, and a screen that prints it turns a glance
 * over a shoulder into a forgery.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountDetailScreen(detail: AccountDetail, vm: MailViewModel) {
    val theme = LocalTheme.current
    // Kept across a rotation, like every other dialog in this app.
    var editingQuota by rememberSaveable { mutableStateOf(false) }
    if (editingQuota) {
        QuotaDialog(
            current = detail.quotaBytes,
            onDismiss = { editingQuota = false },
            onConfirm = { bytes ->
                editingQuota = false
                vm.setAccountQuota(detail.address, bytes)
            },
        )
    }

    Scaffold(
        containerColor = theme.bg,
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        detail.address,
                        fontSize = 16.sp,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = theme.bg,
                    titleContentColor = theme.fg,
                ),
                navigationIcon = {
                    IconButton(
                        onClick = { vm.closeAccount() },
                        modifier = Modifier.testTag("button.closeAccount"),
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
            if (detail.loading) {
                CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)
                return@Box
            }
            Column(
                Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .testTag("account.detail"),
            ) {
                Heading("Quota")
                // The whole row opens the editor, not just the icon:
                // a 48dp icon beside a line of text is the smaller
                // target, and the text is what an operator is looking
                // at when they decide to change it.
                // One tag, because this is one thing: `clickable`
                // merges its descendants' semantics, so a tag on the
                // value inside would be folded into the row and could
                // not be found by tag at all. The row is the value and
                // the button both, and the test reads its text and
                // clicks it.
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { editingQuota = true }
                        .padding(horizontal = 16.dp, vertical = 8.dp)
                        .testTag("account.quota"),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        // Null and zero both mean no cap here, and "0 B"
                        // would read as a mailbox that can hold nothing.
                        detail.quotaBytes?.takeIf { it > 0 }?.let(::humanSize) ?: "No limit",
                        color = theme.fg,
                        fontSize = 14.sp,
                    )
                    Spacer(Modifier.weight(1f))
                    Icon(
                        Icons.Filled.Edit,
                        contentDescription = "Change the storage limit",
                        tint = theme.fgSecondary,
                    )
                }

                HorizontalDivider(color = theme.border, thickness = 0.5.dp, modifier = Modifier.padding(top = 12.dp))
                Heading("Sieve")
                if (detail.sieve.isBlank()) {
                    Text(
                        "No script — nothing is filed automatically.",
                        color = theme.fgMuted,
                        fontSize = 13.sp,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                    )
                } else {
                    Text(
                        detail.sieve,
                        color = theme.fg,
                        fontSize = 11.sp,
                        fontFamily = FontFamily.Monospace,
                        softWrap = false,
                        modifier = Modifier
                            .fillMaxWidth()
                            .horizontalScroll(rememberScrollState())
                            .padding(horizontal = 16.dp, vertical = 4.dp)
                            .testTag("account.sieve"),
                    )
                }

                HorizontalDivider(color = theme.border, thickness = 0.5.dp, modifier = Modifier.padding(top = 12.dp))
                Heading("Webhooks")
                if (detail.webhooks.isEmpty()) {
                    Text(
                        "Nothing subscribes to this account.",
                        color = theme.fgMuted,
                        fontSize = 13.sp,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                    )
                } else {
                    for (hook in detail.webhooks) {
                        Column(
                            Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 16.dp, vertical = 6.dp)
                                .testTag("row.webhook"),
                        ) {
                            Text(hook.url, color = theme.fg, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(
                                listOfNotNull(
                                    hook.eventType,
                                    if (hook.active) null else "inactive",
                                    hook.filterSender?.let { "from $it" },
                                ).joinToString(" · "),
                                color = theme.fgMuted,
                                fontSize = 11.sp,
                            )
                        }
                    }
                }
                Box(Modifier.padding(bottom = 24.dp))
            }
        }
    }
}

@Composable
private fun Heading(text: String) {
    val theme = LocalTheme.current
    Text(
        text,
        color = theme.fgMuted,
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
    )
}

/**
 * How much this account may hold, in gigabytes.
 *
 * One field, because that is the whole edit. Clearing it lifts the
 * cap — the hint says so — rather than hiding "no limit" behind a
 * second control that an operator who has already cleared the field
 * would have no reason to look for.
 *
 * Save stays disabled while the text is not a number, so a typo is
 * refused before it travels rather than lifting the cap on the way.
 */
@Composable
private fun QuotaDialog(current: Long?, onDismiss: () -> Unit, onConfirm: (Long) -> Unit) {
    val theme = LocalTheme.current
    var text by rememberSaveable { mutableStateOf(QuotaInput.display(current)) }
    val parsed = QuotaInput.parse(text)
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = theme.surface,
        title = { Text("Storage limit", fontSize = 16.sp, color = theme.fg) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                singleLine = true,
                label = { Text("Gigabytes", fontSize = 13.sp) },
                placeholder = { Text("Empty for no limit", fontSize = 13.sp) },
                keyboardOptions = KeyboardOptions(
                    keyboardType = KeyboardType.Decimal,
                    imeAction = ImeAction.Done,
                ),
                keyboardActions = KeyboardActions(onDone = { parsed?.let(onConfirm) }),
                modifier = Modifier.fillMaxWidth().testTag("field.quota"),
            )
        },
        confirmButton = {
            TextButton(
                onClick = { parsed?.let(onConfirm) },
                enabled = parsed != null,
                modifier = Modifier.testTag("button.saveQuota"),
            ) {
                Text("Save", color = if (parsed == null) theme.fgMuted else theme.accent)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel", color = theme.fgSecondary) }
        },
    )
}
