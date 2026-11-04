package jp.golia.mailrs.ui

import android.app.TimePickerDialog
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.SettingsPage
import jp.golia.mailrs.UiState
import jp.golia.mailrs.closeSettingsPage
import jp.golia.mailrs.loadNotifyOverrides
import jp.golia.mailrs.loadNotifyPrefs
import jp.golia.mailrs.notifySender
import jp.golia.mailrs.notifyThread
import jp.golia.mailrs.saveNotifyPrefs
import jp.golia.mailrs.wire.SettingsWire
import jp.golia.mailrs.wire.Texts
import kotlinx.coroutines.launch
import java.util.TimeZone

/** One settings page, over Settings, with its own way back. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsPageScreen(page: SettingsPage, state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val snackbars = remember { SnackbarHostState() }
    FailureSnackbar(state, vm, snackbars, hasContent = true)
    Scaffold(
        containerColor = theme.bg,
        snackbarHost = { SnackbarHost(snackbars) },
        topBar = {
            TopAppBar(
                title = { Text(Texts.get(titleOf(page)), fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = theme.bg, titleContentColor = theme.fg),
                navigationIcon = {
                    IconButton(onClick = { vm.closeSettingsPage() }, modifier = Modifier.testTag("button.closeSettingsPage")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, Texts.get(R.string.common_back), tint = theme.fgSecondary)
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
            when (page) {
                SettingsPage.Notifications -> NotifyPage(vm)
                SettingsPage.Sessions -> SessionsPage(vm)
                SettingsPage.AppPasswords -> AppPasswordsPage(state, vm)
                SettingsPage.Vacation -> VacationPage(vm)
                SettingsPage.Masked -> MaskedPage(vm)
            }
        }
    }
}

private val modes = listOf(
    "default" to R.string.notify_mode_default,
    "all" to R.string.notify_mode_all,
    "important" to R.string.notify_mode_important,
    "off" to R.string.notify_mode_off,
)

/** Which mail notifies, the quiet hours, and what is set apart from the rule. */
@Composable
private fun NotifyPage(vm: MailViewModel) {
    val theme = LocalTheme.current
    val scope = rememberCoroutineScope()
    var prefs by remember { mutableStateOf<SettingsWire.NotifyPrefs?>(null) }
    var overrides by remember { mutableStateOf<SettingsWire.Overrides?>(null) }
    LaunchedEffect(Unit) {
        prefs = vm.loadNotifyPrefs()
        overrides = vm.loadNotifyOverrides()
    }
    // shown at once, saved behind; a refusal puts the old value back
    fun save(next: SettingsWire.NotifyPrefs) {
        val before = prefs
        val stamped = next.copy(utcOffsetMinutes = TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 60_000)
        prefs = stamped
        scope.launch { if (!vm.saveNotifyPrefs(stamped)) prefs = before }
    }
    val p = prefs ?: return
    for ((wire, label) in modes) {
        // the whole line answers the tap, as Material's radio lists do
        Row(
            Modifier.fillMaxWidth()
                .selectable(selected = p.mode == wire, role = Role.RadioButton, onClick = { save(p.copy(mode = wire)) })
                .padding(horizontal = 8.dp)
                .testTag("notify.mode.$wire"),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(selected = p.mode == wire, onClick = null)
            Text(Texts.get(label), color = theme.fg, fontSize = 14.sp)
        }
    }
    SwitchRow(Texts.get(R.string.notify_hide_content), p.hideContent, "notify.hideContent") { save(p.copy(hideContent = it)) }
    SwitchRow(Texts.get(R.string.notify_quiet_hours), p.quietStart != null, "notify.quiet") { on ->
        save(p.copy(quietStart = QuietHours.start(on), quietEnd = QuietHours.end(on)))
    }
    if (p.quietStart != null && p.quietEnd != null) {
        QuietTime(Texts.get(R.string.notify_quiet_starts), p.quietStart, "notify.quietStart") { save(p.copy(quietStart = it)) }
        QuietTime(Texts.get(R.string.notify_quiet_ends), p.quietEnd, "notify.quietEnd") { save(p.copy(quietEnd = it)) }
        Text(
            Texts.get(R.string.notify_quiet_window, QuietHours.label(p.quietStart), QuietHours.label(p.quietEnd)),
            color = theme.fgMuted,
            fontSize = 12.sp,
            modifier = Modifier.padding(horizontal = 16.dp).testTag("notify.quietWindow"),
        )
    }
    val o = overrides ?: return
    OverrideList(Texts.get(R.string.notify_senders), o.senders, "notify.sender") { key ->
        scope.launch { if (vm.notifySender(key, null)) overrides = vm.loadNotifyOverrides() }
    }
    OverrideList(Texts.get(R.string.notify_threads), o.threads, "notify.thread") { key ->
        scope.launch { if (vm.notifyThread(key, null)) overrides = vm.loadNotifyOverrides() }
    }
}

@Composable
private fun OverrideList(title: String, rows: List<List<String>>, tag: String, remove: (String) -> Unit) {
    if (rows.isEmpty()) return
    val theme = LocalTheme.current
    Text(title, color = theme.fgMuted, fontSize = 12.sp, modifier = Modifier.padding(start = 16.dp, top = 16.dp))
    for (row in rows) {
        val key = row.firstOrNull().orEmpty()
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp).testTag("$tag.$key"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(key, color = theme.fg, fontSize = 14.sp)
                Text(Texts.get(overrideLabel(row.lastOrNull().orEmpty())), color = theme.fgMuted, fontSize = 12.sp)
            }
            TextButton(onClick = { remove(key) }, modifier = Modifier.testTag("$tag.$key.remove")) {
                Text(Texts.get(R.string.notify_remove), fontSize = 13.sp)
            }
        }
    }
}

private fun overrideLabel(value: String): Int = when (value) {
    "always" -> R.string.notify_always
    "never" -> R.string.notify_never
    "follow" -> R.string.notify_follow
    else -> R.string.notify_muted
}

/** One end of the quiet hours; a tap opens the system's time picker. */
@Composable
private fun QuietTime(label: String, minutes: Int, tag: String, onPick: (Int) -> Unit) {
    val context = LocalContext.current
    TextButton(
        onClick = {
            TimePickerDialog(context, { _, h, m -> onPick(h * 60 + m) }, minutes / 60, minutes % 60, true).show()
        },
        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp).testTag(tag),
    ) {
        Text(label, color = LocalTheme.current.fg, fontSize = 14.sp, modifier = Modifier.weight(1f))
        Text(QuietHours.label(minutes), color = LocalTheme.current.accent, fontSize = 14.sp)
    }
}

@Composable
internal fun SwitchRow(label: String, checked: Boolean, tag: String, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = LocalTheme.current.fg, fontSize = 14.sp, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = onChange, modifier = Modifier.testTag(tag))
    }
}

/** Quiet hours as minutes after midnight, the server's unit; the same defaults as the iPhone. */
object QuietHours {
    fun start(on: Boolean): Int? {
        if (on) return 22 * 60
        return null
    }

    fun end(on: Boolean): Int? {
        if (on) return 7 * 60
        return null
    }

    fun label(minutes: Int): String = "%02d:%02d".format(minutes / 60, minutes % 60)
}
