package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.loadUnifiedPush
import jp.golia.mailrs.removePushDevice
import jp.golia.mailrs.wire.PushDistributor
import jp.golia.mailrs.wire.PushRegistration
import jp.golia.mailrs.wire.PushStatus
import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.wire.UnifiedPushWire
import kotlinx.coroutines.launch

/**
 * Instant notifications through a UnifiedPush distributor: which one, on
 * or off, and every phone of this account that gets pushes. Without a
 * distributor the page says plainly what the app does instead.
 */
@Composable
internal fun PushPage(vm: MailViewModel) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val reg = remember { PushRegistration(context) }
    // the distributor and the server answer in the background: redraw when either half changes
    var changes by remember { mutableIntStateOf(0) }
    DisposableEffect(reg) {
        val l = reg.listen { changes++ }
        onDispose { reg.unlisten(l) }
    }
    val have = remember(changes) { reg.snapshot() }
    var server by remember { mutableStateOf<UnifiedPushWire.State?>(null) }
    LaunchedEffect(have.serverId) { server = vm.loadUnifiedPush() }
    val installed = remember(changes) { PushDistributor.installed(context) }
    val chosen = remember(changes) { PushDistributor.chosen(context) }
    val s = server ?: return
    val status = PushStatus.of(s.enabled, installed, chosen, have, reg.failure)

    Note(Texts.get(R.string.push_intro), "push.intro")
    when (status) {
        PushStatus.ServerOff -> Note(Texts.get(R.string.push_server_off), "push.serverOff")
        PushStatus.NoDistributor -> {
            Note(Texts.get(R.string.push_none_installed), "push.noDistributor")
            Note(Texts.get(R.string.push_none_hint), "push.noDistributorHint")
        }
        else -> Control(status, installed, s.vapidPublicKey.orEmpty())
    }
    Devices(s.devices, have.serverId) { id ->
        scope.launch { if (vm.removePushDevice(id)) server = vm.loadUnifiedPush() }
    }
}

@Composable
private fun Control(status: PushStatus, installed: List<String>, vapid: String) {
    val context = LocalContext.current
    val theme = LocalTheme.current
    var pick by remember(installed) { mutableStateOf(installed.first()) }
    Text(
        statusText(context, status),
        color = theme.fg,
        fontSize = 14.sp,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("push.status"),
    )
    val off = status is PushStatus.Off || status is PushStatus.Failed
    if (!off) {
        OutlinedButton(
            onClick = { PushDistributor.stop(context) },
            modifier = Modifier.padding(horizontal = 16.dp).testTag("push.turnOff"),
        ) { Text(Texts.get(R.string.push_turn_off)) }
        return
    }
    if (installed.size > 1) {
        Text(Texts.get(R.string.push_pick), color = theme.fgMuted, fontSize = 12.sp, modifier = Modifier.padding(start = 16.dp, top = 8.dp))
        for (pkg in installed) {
            Row(
                Modifier.fillMaxWidth()
                    .selectable(selected = pick == pkg, role = Role.RadioButton, onClick = { pick = pkg })
                    .padding(horizontal = 8.dp)
                    .testTag("push.distributor.$pkg"),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = pick == pkg, onClick = null)
                Text(PushDistributor.label(context, pkg), color = theme.fg, fontSize = 14.sp)
            }
        }
    }
    Button(
        onClick = { PushDistributor.use(context, pick, vapid) },
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("push.turnOn"),
    ) { Text(Texts.get(R.string.push_turn_on)) }
}

private fun statusText(context: android.content.Context, status: PushStatus): String = when (status) {
    is PushStatus.On -> Texts.get(R.string.push_status_on, PushDistributor.label(context, status.pkg))
    is PushStatus.Waiting -> Texts.get(R.string.push_status_waiting, PushDistributor.label(context, status.pkg))
    is PushStatus.Failed -> Texts.get(R.string.push_status_failed, failureText(status.reason))
    PushStatus.Off, PushStatus.ServerOff, PushStatus.NoDistributor -> Texts.get(R.string.push_status_off)
}

/** The distributor's reasons by name; anything else is the server's own words. */
private fun failureText(reason: String): String = when (reason) {
    "NETWORK" -> Texts.get(R.string.push_failed_network)
    "ACTION_REQUIRED" -> Texts.get(R.string.push_failed_action)
    "VAPID_REQUIRED" -> Texts.get(R.string.push_failed_vapid)
    "INTERNAL_ERROR" -> Texts.get(R.string.push_failed_internal)
    else -> reason
}

@Composable
private fun Devices(devices: List<UnifiedPushWire.Device>, thisPhone: String?, remove: (String) -> Unit) {
    if (devices.isEmpty()) return
    val theme = LocalTheme.current
    Text(Texts.get(R.string.push_devices), color = theme.fgMuted, fontSize = 12.sp, modifier = Modifier.padding(start = 16.dp, top = 16.dp))
    for (d in devices) {
        val mine = d.id == thisPhone
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp).testTag("push.device.${d.id}"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(deviceName(d, mine), color = theme.fg, fontSize = 14.sp)
                Text(d.host, color = theme.fgMuted, fontSize = 12.sp)
            }
            // this phone is turned off above, which tells its distributor too
            if (!mine) {
                TextButton(onClick = { remove(d.id) }, modifier = Modifier.testTag("push.device.${d.id}.remove")) {
                    Text(Texts.get(R.string.push_remove), fontSize = 13.sp)
                }
            }
        }
    }
}

private fun deviceName(d: UnifiedPushWire.Device, mine: Boolean): String {
    if (mine) return Texts.get(R.string.push_this_phone, d.name)
    return d.name
}

@Composable
private fun Note(text: String, tag: String) {
    Text(
        text,
        color = LocalTheme.current.fgMuted,
        fontSize = 13.sp,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp).testTag(tag),
    )
}
