package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.R
import jp.golia.mailrs.wire.SavedServer
import jp.golia.mailrs.wire.ServerAddress
import jp.golia.mailrs.wire.ServerList
import jp.golia.mailrs.wire.Texts

/**
 * Adding a server, or changing one the reader added: a name and an
 * address. [existing] is null for a new one.
 */
@Composable
fun ServerEditDialog(
    servers: ServerList,
    existing: SavedServer?,
    onSave: (name: String, address: String) -> Unit,
    onDelete: () -> Unit,
    onDismiss: () -> Unit,
) {
    val theme = LocalTheme.current
    var name by rememberSaveable { mutableStateOf(existing?.name.orEmpty()) }
    var address by rememberSaveable {
        mutableStateOf(existing?.address?.let(ServerAddress::display).orEmpty())
    }
    val problem = problemWith(servers, existing, name, address)
    val typed = address.isNotBlank()
    val ready = typed && problem == null
    fun submit() {
        if (ready) onSave(name, address)
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = theme.surface,
        title = {
            Text(
                when (existing) {
                    null -> Texts.get(R.string.servers_add)
                    else -> Texts.get(R.string.servers_edit)
                },
                fontSize = 18.sp,
                color = theme.fg,
            )
        },
        text = {
            Column {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text(Texts.get(R.string.servers_name)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(
                        capitalization = KeyboardCapitalization.Words,
                        imeAction = ImeAction.Next,
                    ),
                    modifier = Modifier.fillMaxWidth().testTag("server.field.name"),
                )
                OutlinedTextField(
                    value = address,
                    onValueChange = { address = it },
                    label = { Text(Texts.get(R.string.common_address)) },
                    placeholder = { Text("mail.example.com") },
                    singleLine = true,
                    // said once something is typed, not while the field is empty
                    isError = typed && problem != null,
                    supportingText = {
                        Text(
                            when {
                                !typed || problem == null -> Texts.get(R.string.servers_address_help)
                                problem == ServerList.Problem.Duplicate -> Texts.get(R.string.servers_problem_duplicate)
                                else -> Texts.get(R.string.servers_problem_address)
                            },
                            modifier = Modifier.testTag("server.addressNote"),
                        )
                    },
                    keyboardOptions = addressKeyboard(imeAction = ImeAction.Done, keyboardType = KeyboardType.Uri),
                    keyboardActions = KeyboardActions(onDone = { submit() }),
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp).testTag("server.field.address"),
                )
                if (existing != null) {
                    TextButton(
                        onClick = onDelete,
                        // a text button insets its label; pulled back so it starts where the fields do
                        modifier = Modifier.padding(top = 4.dp).offset(x = (-12).dp).testTag("server.delete"),
                    ) { Text(Texts.get(R.string.servers_delete), color = theme.danger) }
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = ready,
                onClick = { submit() },
                modifier = Modifier.testTag("server.save"),
            ) { Text(Texts.get(R.string.common_save), color = saveColour(ready, theme)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, modifier = Modifier.testTag("server.cancel")) {
                Text(Texts.get(R.string.common_cancel), color = theme.fgSecondary)
            }
        },
    )
}

private fun saveColour(ready: Boolean, theme: Theme) = when (ready) {
    true -> theme.accent
    false -> theme.fgMuted
}

/** What is wrong with the address as typed; null when it can be saved. */
private fun problemWith(
    servers: ServerList,
    existing: SavedServer?,
    name: String,
    address: String,
): ServerList.Problem? = try {
    when (existing) {
        null -> servers.add(name, address)
        else -> servers.edit(existing.id, name, address)
    }
    null
} catch (refused: ServerList.Refused) {
    refused.problem
}
