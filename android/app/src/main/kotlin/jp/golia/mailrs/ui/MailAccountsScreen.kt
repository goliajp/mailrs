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
import androidx.compose.foundation.clickable
import androidx.compose.material3.Checkbox
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch

import jp.golia.mailrs.wire.ManualEndpoint
import jp.golia.mailrs.wire.accountSubtitle
import jp.golia.mailrs.wire.connectAccount
import jp.golia.mailrs.wire.wireEndpoints
import jp.golia.mailrs.wire.disconnectAccount
import jp.golia.mailrs.wire.setAccountPaused
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
    // Shut unless somebody opens it: a form that opens with eight
    // empty boxes teaches everybody that connecting mail is hard.
    var manual by remember { mutableStateOf(false) }
    var incoming by remember { mutableStateOf(ManualEndpoint(proto = "imap")) }
    var outgoing by remember { mutableStateOf(ManualEndpoint(proto = "smtp")) }
    var login by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    // A failure here is not an empty list: it said nothing and showed
    // nothing, so a decoding fault was indistinguishable from having
    // connected no mailboxes.
    suspend fun reload() {
        when (val r = client.externalAccounts()) {
            is MailrsClient.Outcome.Ok -> {
                accounts = r.value
                failure = ""
            }
            is MailrsClient.Outcome.Err -> {
                accounts = emptyList()
                failure = r.message
            }
        }
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
            AccountRow(
                a,
                onPause = {
                    busy = true
                    failure = ""
                    val out = client.setAccountPaused(a.id, a.state != "paused")
                    if (out is MailrsClient.Outcome.Err) failure = out.message
                    reload()
                    busy = false
                },
                onRemove = {
                    busy = true
                    failure = ""
                    val out = client.disconnectAccount(a.id)
                    if (out is MailrsClient.Outcome.Err) failure = out.message
                    reload()
                    busy = false
                },
            )
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
                preset?.auth == "oauth2" -> Column(
                    Modifier.testTag("account.oauthUnavailable")
                ) {
                    Text(
                        "${preset.label} does not accept a password for mail apps.",
                        color = theme.fgMuted,
                        fontSize = 12.sp,
                    )
                    // Said here rather than discovered at the end of a
                    // sign-in that could not have finished.
                    Text(
                        "This server cannot connect ${preset.label} accounts yet.",
                        color = theme.fgMuted,
                        fontSize = 12.sp,
                    )
                }
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
                // The whole row, not just the box: tapping the words
                // that name a checkbox is what everybody does, and it
                // did nothing.
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { manual = !manual }
                        .testTag("account.manual"),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = manual, onCheckedChange = { manual = it })
                    Text("Enter the server settings myself", color = theme.fgMuted, fontSize = 12.sp)
                }
                if (manual) {
                    EndpointFields("Incoming", incoming, listOf("imap", "pop3", "jmap")) { incoming = it }
                    EndpointFields("Outgoing", outgoing, listOf("smtp")) { outgoing = it }
                    OutlinedTextField(
                        value = login,
                        onValueChange = { login = it },
                        label = { Text("Login name, if it is not the address") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth().testTag("account.login"),
                    )
                }
                Button(
                    onClick = {
                        // This was an empty lambda: the button was
                        // there, it looked enabled, and pressing it
                        // did nothing at all.
                        val servers = when {
                            manual -> wireEndpoints(incoming, outgoing)
                            else -> null
                        }
                        when {
                            manual && servers == null ->
                                failure = "Both servers need a name and a port"
                            else -> scope.launch {
                                busy = true
                                failure = ""
                                val out = client.connectAccount(
                                    email = email,
                                    secret = secret,
                                    name = name,
                                    servers = servers,
                                    login = if (manual) login else "",
                                )
                                when (out) {
                                    is MailrsClient.Outcome.Err -> failure = out.message
                                    else -> {
                                        email = ""
                                        secret = ""
                                        name = ""
                                        login = ""
                                        manual = false
                                        incoming = ManualEndpoint(proto = "imap")
                                        outgoing = ManualEndpoint(proto = "smtp")
                                        reload()
                                    }
                                }
                                busy = false
                            }
                        }
                    },
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

/** One server's boxes, when autodiscovery cannot reach it. */
@Composable
private fun EndpointFields(
    label: String,
    endpoint: ManualEndpoint,
    protocols: List<String>,
    onChange: (ManualEndpoint) -> Unit,
) {
    val theme = LocalTheme.current
    val tag = label.lowercase()
    OutlinedTextField(
        value = endpoint.host,
        onValueChange = { onChange(endpoint.copy(host = it)) },
        label = { Text("$label server") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth().testTag("account.$tag.host"),
    )
    OutlinedTextField(
        value = endpoint.port,
        onValueChange = { onChange(endpoint.copy(port = it)) },
        label = { Text("Port") },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = Modifier.fillMaxWidth().testTag("account.$tag.port"),
    )
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        if (protocols.size > 1) {
            for (p in protocols) {
                Text(
                    p.uppercase(),
                    color = if (endpoint.proto == p) theme.fg else theme.fgMuted,
                    fontSize = 12.sp,
                    modifier = Modifier
                        .clickable { onChange(endpoint.copy(proto = p)) }
                        .testTag("account.$tag.protocol.$p"),
                )
            }
        }
    }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        for ((value, shown) in TLS_CHOICES) {
            Text(
                shown,
                color = if (endpoint.tls == value) theme.fg else theme.fgMuted,
                fontSize = 12.sp,
                modifier = Modifier
                    .clickable { onChange(endpoint.copy(tls = value)) }
                    .testTag("account.$tag.tls.$value"),
            )
        }
    }
}

private val TLS_CHOICES = listOf(
    "implicit" to "TLS",
    "starttls" to "STARTTLS",
    "none" to "None",
)

@Composable
private fun AccountRow(
    a: ExternalAccount,
    onPause: suspend () -> Unit,
    onRemove: suspend () -> Unit,
) {
    val scope = rememberCoroutineScope()
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
            // Only when it says something the line above did not: an
            // account with no name showed its address twice.
            accountSubtitle(a.displayName, a.email)?.let {
                Text(it, color = theme.fgMuted, fontSize = 12.sp)
            }
            // The reason, on the screen somebody actually reads.
            val why = a.lastError.orEmpty()
            if (a.state != "ok" && why.isNotEmpty()) {
                Text(
                    if (why.length > 200) why.take(200) + "…" else why,
                    color = theme.fgMuted,
                    fontSize = 11.sp,
                    modifier = Modifier.testTag("account.why.${a.id}"),
                )
            }
            // Work, not a fault: a re-read after the server renumbered
            // a folder takes as long as the mailbox is big, and silence
            // for that long reads as a stall.
            val note = a.progress.orEmpty()
            if (note.isNotEmpty()) {
                Text(
                    note,
                    color = theme.fgMuted,
                    fontSize = 11.sp,
                    modifier = Modifier.testTag("account.progress.${a.id}"),
                )
            }
        }
        // Not offered for a rejected credential: pausing cannot fix
        // that, and resuming would put it back on a timer that cannot
        // succeed.
        if (a.state != "needs_auth") {
            Text(
                if (a.state == "paused") "Resume" else "Pause",
                color = theme.accent,
                fontSize = 11.sp,
                modifier = Modifier
                    .clickable { scope.launch { onPause() } }
                    .testTag("account.pause.${a.id}"),
            )
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
