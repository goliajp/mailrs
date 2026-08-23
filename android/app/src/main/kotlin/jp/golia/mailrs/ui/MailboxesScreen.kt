package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.accounts.AccountConnection
import jp.golia.mailrs.accounts.AccountStore
import jp.golia.mailrs.accounts.AccountsDraft
import jp.golia.mailrs.accounts.MailAccount
import jp.golia.mailrs.accounts.MailProvider
import kotlinx.coroutines.launch

/**
 * Mailboxes somewhere else.
 *
 * Adding one is meant to be an address and a secret. The address is
 * enough to know where the servers are; what it cannot know is the
 * secret, and for half the providers the thing to type is not the
 * login password at all but a code generated in their web UI. So the
 * form asks for the address first, looks the provider up, and only
 * then shows a secret field — labelled with the provider's own word
 * for it and with a link to the page that makes one.
 */
@Composable
fun MailboxesScreen() {
    val theme = LocalTheme.current
    val context = LocalContext.current
    val uri = LocalUriHandler.current
    val scope = rememberCoroutineScope()
    val store = remember { AccountStore(context) }

    var accounts by remember { mutableStateOf(store.load()) }
    var draft by remember { mutableStateOf(AccountsDraft()) }
    var busy by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf("") }

    Column(
        Modifier
            .fillMaxSize()
            .background(theme.bg)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text("Connected", color = theme.fgMuted, fontSize = 12.sp)
        if (accounts.isEmpty()) {
            Text("No other mailboxes yet.", color = theme.fgMuted, fontSize = 13.sp)
        }
        for (a in accounts) {
            AccountRow(a) {
                store.remove(a.id)
                accounts = store.load()
            }
        }

        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
        Text("Add a mailbox", color = theme.fgMuted, fontSize = 12.sp)

        OutlinedTextField(
            value = draft.address,
            onValueChange = { draft = draft.copy(address = it) },
            label = { Text("you@example.com") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
            modifier = Modifier.fillMaxWidth().testTag("account.address"),
        )

        if (draft.addressLooksComplete) {
            ProviderNote(
                draft.provider,
                MailProvider.guess(draft.address.substringAfterLast('@', "")),
            ) { uri.openUri(it) }

            // A provider that refuses passwords has nothing to type,
            // so nothing is offered — a field that cannot work is
            // worse than no field.
            if (draft.provider?.auth != MailProvider.AuthKind.OAUTH2) {
                OutlinedTextField(
                    value = draft.secret,
                    onValueChange = { draft = draft.copy(secret = it) },
                    label = { Text(draft.secretLabel) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    modifier = Modifier.fillMaxWidth().testTag("account.secret"),
                )
                OutlinedTextField(
                    value = draft.name,
                    onValueChange = { draft = draft.copy(name = it) },
                    label = { Text("Name it (optional)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("account.name"),
                )

                // A disclosure, not a setting: it reveals fields rather
                // than storing a preference, and it says which way it
                // goes.
                TextButton(
                    onClick = {
                        draft = if (draft.manual) {
                            draft.copy(manual = false)
                        } else {
                            draft.copy(manual = true).prefilled()
                        }
                    },
                    modifier = Modifier.testTag("account.manual"),
                ) {
                    Text(
                        if (draft.manual) {
                            "Discover the servers for me"
                        } else {
                            "Enter the server settings myself"
                        },
                        color = theme.accent,
                        fontSize = 12.sp,
                    )
                }

                if (draft.manual) {
                    ServerField("Incoming server", draft.imapHost, "account.incoming.host") {
                        draft = draft.copy(imapHost = it)
                    }
                    ServerField(
                        "Port", draft.imapPort, "account.incoming.port", numeric = true,
                    ) { draft = draft.copy(imapPort = it) }
                    ServerField("Outgoing server", draft.smtpHost, "account.outgoing.host") {
                        draft = draft.copy(smtpHost = it)
                    }
                    ServerField(
                        "Port", draft.smtpPort, "account.outgoing.port", numeric = true,
                    ) { draft = draft.copy(smtpPort = it) }
                    ServerField(
                        "Login name, if it is not the address", draft.login, "account.login",
                    ) { draft = draft.copy(login = it) }
                }

                Row(verticalAlignment = Alignment.CenterVertically) {
                    Button(
                        onClick = {
                            failure = ""
                            val built = draft.account(accounts.size)
                            val account = built.getOrNull()
                            if (account == null) {
                                failure = built.exceptionOrNull()?.message.orEmpty()
                                return@Button
                            }
                            scope.launch {
                                busy = true
                                // Proved before it is stored: a
                                // credential saved and then found to be
                                // wrong is an account that sits in the
                                // list doing nothing, which is
                                // indistinguishable from having no new
                                // mail.
                                val bad = AccountConnection.verify(account, draft.secret)
                                if (bad == null) {
                                    store.saveSecret(draft.secret, account.id)
                                    store.upsert(account)
                                    accounts = store.load()
                                    draft = AccountsDraft()
                                } else {
                                    failure = "The ${bad.stage.label} refused: ${bad.message}"
                                }
                                busy = false
                            }
                        },
                        enabled = !busy && draft.secret.isNotEmpty(),
                        modifier = Modifier.testTag("account.add"),
                    ) {
                        Text(if (busy) "Checking…" else "Add")
                    }
                    if (busy) {
                        CircularProgressIndicator(
                            Modifier.padding(start = 12.dp).size(18.dp),
                            color = theme.accent,
                        )
                    }
                }
            }
        }

        if (failure.isNotEmpty()) {
            Text(
                failure,
                color = theme.danger,
                fontSize = 12.sp,
                modifier = Modifier.testTag("account.failure"),
            )
        }
    }
}

/** What this provider wants, said before anybody types it. */
@Composable
private fun ProviderNote(
    provider: MailProvider?,
    guess: MailProvider,
    onOpen: (String) -> Unit,
) {
    val theme = LocalTheme.current
    when {
        provider == null ->
            // Shown rather than described. Saying "the usual names are
            // filled in below" while the boxes are shut is a sentence
            // about something the person cannot see — and if the guess
            // is wrong, they find out thirty seconds later from a
            // connection failure instead of now, from reading it.
            Column(Modifier.testTag("account.noPreset")) {
                Text(
                    "No preset for this domain. This will try:",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                )
                Text(
                    "${guess.imapHost}:${guess.imapPort} and " +
                        "${guess.smtpHost}:${guess.smtpPort}",
                    color = theme.fgSecondary,
                    fontSize = 12.sp,
                )
                Text(
                    "Open the settings below if that is not right.",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                )
            }

        provider.auth == MailProvider.AuthKind.OAUTH2 ->
            Column(Modifier.testTag("account.oauthUnavailable")) {
                Text(
                    "${provider.label} does not accept a password for mail apps.",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                )
                Text(
                    "Signing in with ${provider.label} is not built yet — a mailbox that " +
                        "takes an app password works today.",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                )
            }

        provider.secretHelp != null ->
            Column {
                Text(
                    "${provider.label} wants a ${provider.secretHelp.what}, not your login " +
                        "password.",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                )
                Text(
                    "Get one",
                    color = theme.accent,
                    fontSize = 12.sp,
                    textDecoration = TextDecoration.Underline,
                    modifier = Modifier
                        .clickable { onOpen(provider.secretHelp.url) }
                        .testTag("account.getSecret"),
                )
            }
    }
}

@Composable
private fun ServerField(
    label: String,
    value: String,
    tag: String,
    numeric: Boolean = false,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(
            keyboardType = if (numeric) KeyboardType.Number else KeyboardType.Text,
        ),
        modifier = Modifier.fillMaxWidth().testTag(tag),
    )
}

@Composable
private fun AccountRow(a: MailAccount, onRemove: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier.fillMaxWidth().testTag("account.${a.id}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(
            Modifier
                .size(10.dp)
                .clip(CircleShape)
                .background(Color(MailAccount.colourFor(a.id))),
        )
        Column(Modifier.weight(1f)) {
            Text(a.title, color = theme.fg, fontSize = 14.sp)
            // Only when it says something the line above did not.
            a.subtitle?.let { Text(it, color = theme.fgMuted, fontSize = 12.sp) }
        }
        Text(
            if (a.provider == "custom") "IMAP" else a.provider.uppercase(),
            color = theme.fgMuted,
            fontSize = 11.sp,
        )
        TextButton(onClick = onRemove, modifier = Modifier.testTag("account.remove.${a.id}")) {
            Text("Remove", color = theme.danger, fontSize = 12.sp)
        }
    }
}
