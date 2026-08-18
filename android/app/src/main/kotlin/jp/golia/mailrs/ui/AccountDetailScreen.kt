package jp.golia.mailrs.ui

import androidx.compose.runtime.remember
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.AccountDetail
import jp.golia.mailrs.closeAccount
import jp.golia.mailrs.MailViewModel

/**
 * One account, opened.
 *
 * The three things about an account kept somewhere other than its row:
 * how much it may hold, the sieve script that files its mail, and what
 * is subscribed to its events.
 *
 * **All three are read-only.** A sieve script is a program and a phone
 * keyboard is the wrong place to edit one; a quota is a number worth
 * changing deliberately, from a screen with room to say what it costs.
 * Showing them is the useful half — an operator asking "why did that
 * message go to Ops" wants to read the rule, not rewrite it.
 *
 * **The signing secret is never shown.** It is what proves a delivery
 * came from this server, and a screen that prints it turns a glance
 * over a shoulder into a forgery.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountDetailScreen(detail: AccountDetail, vm: MailViewModel) {
    val theme = LocalTheme.current
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
                Text(
                    // Null and zero both mean no cap here, and "0 B"
                    // would read as a mailbox that can hold nothing.
                    detail.quotaBytes?.takeIf { it > 0 }?.let(::humanSize) ?: "No limit",
                    color = theme.fg,
                    fontSize = 14.sp,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp).testTag("account.quota"),
                )

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
