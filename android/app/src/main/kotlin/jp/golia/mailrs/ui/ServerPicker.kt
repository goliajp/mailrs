package jp.golia.mailrs.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Restore
import androidx.compose.material.icons.filled.UnfoldMore
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.R
import jp.golia.mailrs.wire.SavedServer
import jp.golia.mailrs.wire.ServerAddress
import jp.golia.mailrs.wire.ServerList
import jp.golia.mailrs.wire.Texts

/**
 * The sign-in screen's server line: which server, and a way to change it.
 *
 * Drawn as the fields under it are, so the form stays one column of
 * like things; it opens a list instead of a keyboard.
 */
@Composable
fun ServerField(chosen: SavedServer, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val where = ServerAddress.display(chosen.address)
    Box(Modifier.fillMaxWidth()) {
        OutlinedTextField(
            value = chosen.name,
            onValueChange = {},
            // not typed into: disabled keeps the keyboard and the cursor
            // away, and the colours below keep it from looking switched off
            enabled = false,
            readOnly = true,
            singleLine = true,
            label = { Text(Texts.get(R.string.common_server)) },
            supportingText = { Text(where) },
            trailingIcon = { Icon(Icons.Filled.UnfoldMore, contentDescription = null) },
            colors = OutlinedTextFieldDefaults.colors(
                disabledTextColor = scheme.onSurface,
                disabledBorderColor = scheme.outline,
                disabledLabelColor = scheme.onSurfaceVariant,
                disabledSupportingTextColor = scheme.onSurfaceVariant,
                disabledTrailingIconColor = scheme.onSurfaceVariant,
            ),
            modifier = Modifier.fillMaxWidth().clearAndSetSemantics {},
        )
        Box(
            Modifier
                .matchParentSize()
                .clickable(role = Role.Button, onClick = onClick)
                .semantics {
                    contentDescription = Texts.get(R.string.servers_field, chosen.name, where)
                }
                .testTag("field.server"),
        )
    }
}

/** The saved servers: choose one, add one, change or remove one. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerPickerSheet(
    servers: ServerList,
    chosen: SavedServer,
    onChoose: (SavedServer) -> Unit,
    onServers: ((ServerList) -> ServerList) -> Unit,
    onDismiss: () -> Unit,
) {
    val theme = LocalTheme.current
    // which entry the form is for: an id, or NEW; null while it is closed
    var editing by rememberSaveable { mutableStateOf<String?>(null) }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = theme.surface) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(bottom = 24.dp)
                .testTag("sheet.servers"),
        ) {
            Text(
                Texts.get(R.string.servers_title),
                color = theme.fg,
                fontSize = 18.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, bottom = 8.dp),
            )
            for (server in servers.visible) {
                ServerRow(
                    server = server,
                    isChosen = server.id == chosen.id,
                    canHide = servers.canHideBuiltIn,
                    onChoose = {
                        onChoose(server)
                        onDismiss()
                    },
                    onEdit = { editing = server.id },
                    onHide = { onServers { it.remove(server.id) } },
                )
            }
            HorizontalDivider(color = theme.border, modifier = Modifier.padding(vertical = 8.dp))
            SheetAction(Icons.Filled.Add, Texts.get(R.string.servers_add), "server.add") { editing = NEW }
            if (servers.canRestoreBuiltIn) {
                SheetAction(
                    Icons.Filled.Restore,
                    Texts.get(R.string.servers_show_again, ServerList.BUILT_IN.name),
                    "server.restore",
                ) { onServers { it.restoreBuiltIn() } }
            }
            Text(
                Texts.get(R.string.servers_footer),
                color = theme.fgMuted,
                fontSize = 13.sp,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 12.dp),
            )
        }
    }
    editing?.let { target ->
        ServerEditDialog(
            servers = servers,
            existing = servers.added.firstOrNull { it.id == target },
            onSave = { name, address ->
                when (target) {
                    NEW -> {
                        val next = servers.add(name, address)
                        onServers { next }
                        // a server just added is the one to sign in to
                        next.entryAt(address)?.let(onChoose)
                    }
                    else -> onServers { it.edit(target, name, address) }
                }
                editing = null
            },
            onDelete = {
                onServers { it.remove(target) }
                editing = null
            },
            onDismiss = { editing = null },
        )
    }
}

private const val NEW = "new"

@Composable
private fun ServerRow(
    server: SavedServer,
    isChosen: Boolean,
    canHide: Boolean,
    onChoose: () -> Unit,
    onEdit: () -> Unit,
    onHide: () -> Unit,
) {
    val theme = LocalTheme.current
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier
                .weight(1f)
                .clickable(role = Role.RadioButton, onClick = onChoose)
                .semantics { selected = isChosen }
                .padding(start = 20.dp, top = 10.dp, bottom = 10.dp)
                .testTag("server.row.${server.name}"),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // the column is kept when empty, so the names line up
            Box(Modifier.width(36.dp)) {
                if (isChosen) {
                    Icon(
                        Icons.Filled.Check,
                        contentDescription = null,
                        tint = theme.accent,
                        modifier = Modifier.size(22.dp),
                    )
                }
            }
            Column(Modifier.weight(1f)) {
                Text(server.name, color = theme.fg, fontSize = 16.sp)
                Text(ServerAddress.display(server.address), color = theme.fgMuted, fontSize = 13.sp)
            }
        }
        when {
            server.id != ServerList.BUILT_IN.id ->
                IconButton(
                    onClick = onEdit,
                    modifier = Modifier.padding(end = 8.dp).testTag("server.edit.${server.name}"),
                ) {
                    Icon(
                        Icons.Filled.Edit,
                        contentDescription = Texts.get(R.string.servers_edit_named, server.name),
                        tint = theme.fgMuted,
                        modifier = Modifier.size(20.dp),
                    )
                }
            canHide ->
                IconButton(
                    onClick = onHide,
                    modifier = Modifier.padding(end = 8.dp).testTag("server.hideBuiltIn"),
                ) {
                    Icon(
                        Icons.Filled.VisibilityOff,
                        contentDescription = Texts.get(R.string.servers_hide_named, server.name),
                        tint = theme.fgMuted,
                        modifier = Modifier.size(20.dp),
                    )
                }
            else -> Spacer(Modifier.width(8.dp))
        }
    }
}

@Composable
private fun SheetAction(icon: ImageVector, label: String, tag: String, onClick: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 20.dp, vertical = 14.dp)
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.width(36.dp)) {
            Icon(icon, contentDescription = null, tint = theme.accent, modifier = Modifier.size(22.dp))
        }
        Text(label, color = theme.accent, fontSize = 16.sp)
    }
}
