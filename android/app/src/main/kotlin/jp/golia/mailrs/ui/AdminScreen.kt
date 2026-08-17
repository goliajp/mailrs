package jp.golia.mailrs.ui

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
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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
fun AdminScreen(section: MailViewModel.AdminSection, state: MailViewModel.UiState, vm: MailViewModel) {
    val theme = LocalTheme.current
    Scaffold(
        containerColor = theme.bg,
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
                        AdminRow(row) { vm.deleteAdminRow(section, row) }
                        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                    }
                }
            }
        }
    }
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
)

@Composable
private fun AdminRow(row: AdminRow, onDelete: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
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
