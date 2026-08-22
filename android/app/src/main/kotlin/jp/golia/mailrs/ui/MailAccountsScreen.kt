package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.AccountSettings
import jp.golia.mailrs.wire.ExternalAccount
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.accountSettings
import jp.golia.mailrs.wire.colourOf
import jp.golia.mailrs.wire.connectAccount
import jp.golia.mailrs.wire.disconnectAccount
import jp.golia.mailrs.wire.externalAccounts
import jp.golia.mailrs.wire.looksLikeAnAddress

/**
 * Mailboxes somewhere else.
 *
 * Connecting one is an address and a secret. The address is enough to
 * know where Gmail's servers are; what it cannot know is that half the
 * providers refuse the login password and want a code generated in
 * their web UI — so this looks the domain up as the address is typed
 * and labels the secret field with the provider's own word for it.
 */
@Composable
fun MailAccountsScreen(client: MailrsClient) {
    val theme = LocalTheme.current
    val uri = LocalUriHandler.current
    var accounts by remember { mutableStateOf<List<ExternalAccount>>(emptyList()) }
    var email by remember { mutableStateOf("") }
    var secret by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var settings by remember { mutableStateOf<AccountSettings?>(null) }
    var failure by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }

    suspend fun reload() {
        accounts = (client.externalAccounts() as? MailrsClient.Outcome.Ok)?.value.orEmpty()
    }

    LaunchedEffect(Unit) { reload() }
    // A partial address is not a domain: asking about "s", "so", "som"
    // is three requests that cannot answer anything.
    LaunchedEffect(email) {
        settings = when {
            looksLikeAnAddress(email) ->
                (client.accountSettings(email) as? MailrsClient.Outcome.Ok)?.value
            else -> null
        }
    }

    val preset = settings?.preset
    val secretLabel = preset?.secretHelp?.what ?: "Password"

    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text("Connected", color = theme.fgMuted, fontSize = 12.sp)
        if (accounts.isEmpty()) {
            Text("No other accounts connected yet.", color = theme.fgMuted, fontSize = 13.sp)
        }
        for (a in accounts) {
            AccountRow(a) {
                busy = true
                failure = ""
                val out = client.disconnectAccount(a.id)
                if (out is MailrsClient.Outcome.Err) failure = out.message
                reload()
                busy = false
            }
        }

        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
        Text("Connect an account", color = theme.fgMuted, fontSize = 12.sp)
        OutlinedTextField(
            value = email,
            onValueChange = { email = it },
            label = { Text("you@gmail.com") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
            modifier = Modifier.fillMaxWidth().testTag("account.email"),
        )
        if (looksLikeAnAddress(email)) {
            when {
                preset?.auth == "oauth2" -> Text(
                    "${preset.label} does not accept a password for mail apps — " +
                        "connecting it opens a sign-in page.",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                )
                preset?.secretHelp != null -> Column {
                    Text(
                        "${preset.label} wants a ${preset.secretHelp.what}, " +
                            "not your login password.",
                        color = theme.fgMuted,
                        fontSize = 12.sp,
                    )
                    TextButton(onClick = { uri.openUri(preset.secretHelp.url) }) {
                        Text("Get one", color = theme.accent, fontSize = 12.sp)
                    }
                }
                settings?.known == false -> Text(
                    "Its server settings will be discovered from DNS when the account is added.",
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                )
            }
            if (preset?.auth != "oauth2") {
                OutlinedTextField(
                    value = secret,
                    onValueChange = { secret = it },
                    label = { Text(secretLabel) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    modifier = Modifier.fillMaxWidth().testTag("account.secret"),
                )
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Name it (optional)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("account.name"),
                )
                Button(
                    onClick = {},
                    enabled = !busy && secret.isNotEmpty(),
                    modifier = Modifier.testTag("account.connect"),
                ) {
                    Text(if (busy) "Connecting…" else "Connect")
                }
            }
        }
        if (failure.isNotEmpty()) {
            Text(failure, color = theme.danger, fontSize = 12.sp, modifier = Modifier.testTag("account.failure"))
        }
    }
}

@Composable
private fun AccountRow(a: ExternalAccount, onRemove: suspend () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier.fillMaxWidth().testTag("account.${a.id}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(
            Modifier.size(10.dp).clip(CircleShape).background(Color(colourOf(a.colour))),
        )
        Column(Modifier.weight(1f)) {
            Text(a.displayName.ifEmpty { a.email }, color = theme.fg, fontSize = 14.sp)
            Text(a.email, color = theme.fgMuted, fontSize = 12.sp)
        }
        // A broken account has to say so where it was added. Silence
        // means somebody believes they are seeing all their mail when
        // they are not.
        a.trouble?.let {
            Text(
                it,
                color = if (a.state == "needs_auth") theme.warning else theme.danger,
                fontSize = 11.sp,
            )
        }
    }
}
