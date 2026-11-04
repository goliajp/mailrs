package jp.golia.mailrs.ui

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.UiState
import jp.golia.mailrs.createMask
import jp.golia.mailrs.endOtherSessions
import jp.golia.mailrs.endSession
import jp.golia.mailrs.loadAppPasswords
import jp.golia.mailrs.loadMasks
import jp.golia.mailrs.loadSessions
import jp.golia.mailrs.loadVacation
import jp.golia.mailrs.revokeAppPassword
import jp.golia.mailrs.saveVacation
import jp.golia.mailrs.wire.SettingsWire
import jp.golia.mailrs.wire.Texts
import kotlinx.coroutines.launch

/** Where this account is signed in, and signing the other places out. */
@Composable
internal fun SessionsPage(vm: MailViewModel) {
    val theme = LocalTheme.current
    val scope = rememberCoroutineScope()
    var rows by remember { mutableStateOf<List<SettingsWire.SignedInSession>?>(null) }
    LaunchedEffect(Unit) { rows = vm.loadSessions() }
    val shown = rows ?: return
    for (s in shown) {
        Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 6.dp).testTag("session.${s.id}"), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(s.userAgent.ifBlank { Texts.get(R.string.sessions_unknown_device) }, color = theme.fg, fontSize = 14.sp)
                Text(sessionLine(s), color = theme.fgMuted, fontSize = 12.sp)
            }
            if (!s.current) {
                TextButton(onClick = { scope.launch { if (vm.endSession(s.id)) rows = vm.loadSessions() } }, modifier = Modifier.testTag("session.${s.id}.end")) {
                    Text(Texts.get(R.string.sessions_sign_out), color = theme.danger, fontSize = 13.sp)
                }
            }
        }
    }
    if (shown.any { !it.current }) {
        TextButton(
            onClick = { scope.launch { if (vm.endOtherSessions()) rows = vm.loadSessions() } },
            modifier = Modifier.padding(8.dp).testTag("sessions.endOthers"),
        ) {
            Text(Texts.get(R.string.sessions_end_others), color = theme.danger, fontSize = 14.sp)
        }
    }
}

private fun sessionLine(s: SettingsWire.SignedInSession): String {
    val seen = RowDate.format(s.lastSeen)
    if (s.current) return Texts.get(R.string.sessions_this_device_line, s.ip, seen)
    return Texts.get(R.string.sessions_line, s.ip, seen)
}

/** App passwords for mail programs: listed and revoked here, made on the web. */
@Composable
internal fun AppPasswordsPage(state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var rows by remember { mutableStateOf<List<SettingsWire.AppPassword>?>(null) }
    LaunchedEffect(Unit) { rows = vm.loadAppPasswords() }
    val shown = rows ?: return
    if (shown.isEmpty()) {
        Text(Texts.get(R.string.app_passwords_none), color = theme.fgMuted, fontSize = 13.sp, modifier = Modifier.padding(16.dp))
    }
    for (p in shown) {
        Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 6.dp).testTag("appPassword.${p.id}"), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(p.name, color = theme.fg, fontSize = 14.sp)
                Text(lastUsed(p.lastUsedAt), color = theme.fgMuted, fontSize = 12.sp)
            }
            TextButton(onClick = { scope.launch { if (vm.revokeAppPassword(p.id)) rows = vm.loadAppPasswords() } }, modifier = Modifier.testTag("appPassword.${p.id}.revoke")) {
                Text(Texts.get(R.string.app_passwords_revoke), color = theme.danger, fontSize = 13.sp)
            }
        }
    }
    TextButton(
        onClick = {
            val page = SettingsWire.appPasswordsPage(state.server)
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(page)))
        },
        modifier = Modifier.padding(8.dp).testTag("appPasswords.web"),
    ) {
        Text(Texts.get(R.string.app_passwords_web), color = theme.accent, fontSize = 14.sp)
    }
    Text(Texts.get(R.string.app_passwords_web_hint), color = theme.fgMuted, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 16.dp))
}

private fun lastUsed(at: Long?): String {
    if (at == null) return Texts.get(R.string.app_passwords_never_used)
    return Texts.get(R.string.app_passwords_last_used, RowDate.format(at))
}

/** The vacation reply, on or off; its text and dates are written on the web. */
@Composable
internal fun VacationPage(vm: MailViewModel) {
    val theme = LocalTheme.current
    val scope = rememberCoroutineScope()
    var vacation by remember { mutableStateOf<SettingsWire.Vacation?>(null) }
    LaunchedEffect(Unit) { vacation = vm.loadVacation() }
    val v = vacation ?: return
    if (v.canTurnOn || v.enabled) {
        SwitchRow(Texts.get(R.string.vacation_enabled), v.enabled, "vacation.enabled") { on ->
            // the whole reply as read, with only `enabled` changed
            scope.launch { vm.saveVacation(v.copy(enabled = on))?.let { vacation = it } }
        }
    }
    if (!v.canTurnOn) {
        Text(Texts.get(R.string.vacation_needs_text), color = theme.fgMuted, fontSize = 13.sp, modifier = Modifier.padding(16.dp).testTag("vacation.needsText"))
        return
    }
    Text(Texts.get(R.string.vacation_interval, v.days), color = theme.fgMuted, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 16.dp))
    if (v.subject.isNotBlank()) Text(v.subject, color = theme.fg, fontSize = 15.sp, modifier = Modifier.padding(start = 16.dp, top = 12.dp))
    Text(v.body, color = theme.fgSecondary, fontSize = 14.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
}

/** Addresses that forward here and can be turned off once they leak. */
@Composable
internal fun MaskedPage(vm: MailViewModel) {
    val theme = LocalTheme.current
    val scope = rememberCoroutineScope()
    var masks by remember { mutableStateOf<List<SettingsWire.Mask>?>(null) }
    var site by remember { mutableStateOf("") }
    LaunchedEffect(Unit) { masks = vm.loadMasks() }
    OutlinedTextField(
        value = site,
        onValueChange = { site = it },
        label = { Text(Texts.get(R.string.masked_for_site)) },
        singleLine = true,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("masked.forDomain"),
    )
    TextButton(
        onClick = {
            scope.launch {
                val made = vm.createMask(site.trim()) ?: return@launch
                site = ""
                masks = listOf(made) + masks.orEmpty()
            }
        },
        modifier = Modifier.padding(horizontal = 8.dp).testTag("masked.create"),
    ) {
        Text(Texts.get(R.string.masked_create), color = theme.accent, fontSize = 14.sp)
    }
    val shown = masks ?: return
    if (shown.isEmpty()) {
        Text(Texts.get(R.string.masked_none), color = theme.fgMuted, fontSize = 13.sp, modifier = Modifier.padding(16.dp))
    }
    for (m in shown) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp).testTag("mask.${m.id}")) {
            Text(m.email, color = theme.fg, fontSize = 14.sp)
            Text(m.forDomain?.ifBlank { null } ?: m.description.orEmpty(), color = theme.fgMuted, fontSize = 12.sp)
        }
    }
}
