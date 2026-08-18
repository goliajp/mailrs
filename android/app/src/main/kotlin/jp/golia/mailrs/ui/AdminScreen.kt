package jp.golia.mailrs.ui

import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch
import androidx.compose.ui.platform.LocalClipboard
import jp.golia.mailrs.newAgentKeySeen
import androidx.compose.foundation.clickable
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.DeleteOutline
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.TextButton
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.AdminSection
import jp.golia.mailrs.UiState
import jp.golia.mailrs.addAdminRow
import jp.golia.mailrs.addFields
import jp.golia.mailrs.closeAdmin
import jp.golia.mailrs.deleteAdminRow
import jp.golia.mailrs.openAdminRow
import jp.golia.mailrs.MailViewModel

/**
 * The operator's screens: who has an account, what forwards where, which
 * domains this server answers for.
 *
 * **One shell, three lists.** They differ in what a row says and what
 * deleting one means; everything else — the bar, the spinner, the empty
 * sentence, the failure — is the same, and writing it three times is how
 * three screens end up disagreeing about what "loading" looks like.
 *
 * Read-only for accounts. Creating one takes a password, and a password
 * field on a phone that also holds the session is a decision worth
 * making deliberately rather than in passing; aliases and domains have
 * no such weight and can be added and removed here.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AdminScreen(section: AdminSection, state: UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    val fields = vm.addFields(section)
    // Kept across a rotation, like every other dialog here.
    var adding by rememberSaveable { mutableStateOf(false) }

    if (adding) {
        AddRowDialog(
            title = section.title,
            fields = fields,
            onDismiss = { adding = false },
            onConfirm = { values ->
                adding = false
                vm.addAdminRow(section, values)
            },
        )
    }

    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    // The one moment the secret exists where it can be read: the list
    // returns a prefix and the server keeps a hash. A dialog rather
    // than a snackbar, because this one has to be dismissed on
    // purpose — a message that times out while somebody is copying it
    // is the same as never showing it.
    state.newAgentKey?.let { secret ->
        AlertDialog(
            onDismissRequest = { vm.newAgentKeySeen() },
            containerColor = theme.surface,
            title = { Text("Copy this key now", fontSize = 16.sp, color = theme.fg) },
            text = {
                Column {
                    Text(
                        secret,
                        fontSize = 14.sp,
                        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                        color = theme.fg,
                        modifier = Modifier.testTag("text.newAgentKey"),
                    )
                    Text(
                        "It is not shown again — the server keeps only a hash.",
                        fontSize = 12.sp,
                        color = theme.fgMuted,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        scope.launch {
                            clipboard.setClipEntry(
                                androidx.compose.ui.platform.ClipEntry(
                                    android.content.ClipData.newPlainText("Agent key", secret),
                                ),
                            )
                        }
                        vm.newAgentKeySeen()
                    },
                    modifier = Modifier.testTag("button.copyAgentKey"),
                ) {
                    Text("Copy", color = theme.accent)
                }
            },
            dismissButton = {
                TextButton(onClick = { vm.newAgentKeySeen() }) {
                    Text("Done", color = theme.fgSecondary)
                }
            },
        )
    }

    val snackbars = remember { SnackbarHostState() }
    FailureSnackbar(state, vm, snackbars, hasContent = section.rows(state).isNotEmpty())

    Scaffold(
        containerColor = theme.bg,
        snackbarHost = { SnackbarHost(snackbars) },
        floatingActionButton = {
            if (fields.isNotEmpty()) {
                FloatingActionButton(
                    onClick = { adding = true },
                    containerColor = theme.accent,
                    contentColor = theme.accentFg,
                    modifier = Modifier.testTag("button.addAdminRow"),
                ) {
                    Icon(Icons.Filled.Add, contentDescription = "Add")
                }
            }
        },
        topBar = {
            TopAppBar(
                title = { Text(section.title, fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = theme.bg,
                    titleContentColor = theme.fg,
                ),
                navigationIcon = {
                    IconButton(
                        onClick = { vm.closeAdmin() },
                        modifier = Modifier.testTag("button.closeAdmin"),
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
            val rows = section.rows(state)
            when {
                state.busy && rows.isEmpty() ->
                    CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)

                state.error != null && rows.isEmpty() ->
                    Text(
                        state.error,
                        color = theme.danger,
                        fontSize = 13.sp,
                        modifier = Modifier.align(Alignment.Center).padding(32.dp).testTag("admin.error"),
                    )

                rows.isEmpty() ->
                    Text(
                        section.emptyMessage,
                        color = theme.fgMuted,
                        fontSize = 13.sp,
                        modifier = Modifier.align(Alignment.Center).padding(32.dp).testTag("admin.empty"),
                    )

                else -> LazyColumn(Modifier.fillMaxSize().testTag("list.admin")) {
                    items(rows, key = { it.key }) { row ->
                        AdminRow(
                            row = row,
                            onOpen = if (row.drillable) {
                                { vm.openAdminRow(section, row) }
                            } else {
                                null
                            },
                            onDelete = { vm.deleteAdminRow(section, row) },
                        )
                        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                    }
                }
            }
        }
    }
}

/**
 * The form for a new row.
 *
 * An `AlertDialog`, which is Android's shape for a short interruption
 * with a decision at the end of it, rather than another screen: two
 * fields do not deserve a navigation.
 */
@Composable
private fun AddRowDialog(
    title: String,
    fields: List<String>,
    onDismiss: () -> Unit,
    onConfirm: (List<String>) -> Unit,
) {
    val theme = LocalTheme.current
    val values = remember(fields) { mutableStateListOf(*Array(fields.size) { "" }) }
    val ready = values.all { it.isNotBlank() }
    fun submit() {
        if (ready) onConfirm(values.toList())
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = theme.surface,
        title = { Text("New ${title.trimEnd('s').lowercase()}", fontSize = 16.sp, color = theme.fg) },
        text = {
            Column {
                fields.forEachIndexed { i, label ->
                    OutlinedTextField(
                        value = values[i],
                        onValueChange = { values[i] = it },
                        label = { Text(label, fontSize = 13.sp) },
                        singleLine = true,
                        // The last field's key adds; the others move on.
                        // A dialog whose keyboard offers a newline in a
                        // one-line field is asking to be dismissed
                        // before the button can be reached.
                        keyboardOptions = KeyboardOptions(
                            imeAction = if (i == fields.lastIndex) ImeAction.Done else ImeAction.Next,
                        ),
                        keyboardActions = KeyboardActions(onDone = { submit() }),
                        modifier = Modifier.fillMaxWidth().testTag("field.admin$i"),
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                // Every field, not just the first: a half-filled form
                // that the server answers 400 to is a worse answer than
                // a button that has not lit up yet.
                enabled = ready,
                onClick = { submit() },
                modifier = Modifier.testTag("button.confirmAdmin"),
            ) {
                Text("Add", color = theme.accent)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel", color = theme.fgSecondary) }
        },
    )
}

/**
 * One row of an operator list.
 *
 * `key` is the identity the server knows the thing by — an alias id, a
 * domain name, an address — and is what a delete names. Keeping it here
 * rather than deriving it from the headline means a row whose display
 * text changes still deletes the right thing.
 */
data class AdminRow(
    val key: String,
    val headline: String,
    val detail: String,
    val deletable: Boolean,
    /** Whether there is something inside it worth opening. */
    val drillable: Boolean = false,
)

@Composable
private fun AdminRow(row: AdminRow, onOpen: (() -> Unit)?, onDelete: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
            .then(if (onOpen != null) Modifier.clickable(onClick = onOpen) else Modifier)
            .padding(start = 16.dp, end = 4.dp, top = 12.dp, bottom = 12.dp)
            .testTag("row.admin"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                row.headline,
                color = theme.fg,
                fontSize = 14.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (row.detail.isNotEmpty()) {
                Text(
                    row.detail,
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (row.deletable) {
            IconButton(onClick = onDelete, modifier = Modifier.testTag("button.deleteAdminRow")) {
                Icon(
                    Icons.Filled.DeleteOutline,
                    contentDescription = "Delete ${row.headline}",
                    tint = theme.fgMuted,
                    modifier = Modifier.size(18.dp),
                )
            }
        }
    }
}
