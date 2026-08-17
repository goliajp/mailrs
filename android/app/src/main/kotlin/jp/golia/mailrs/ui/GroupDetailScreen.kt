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
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.DeleteOutline
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.AdminDetail
import jp.golia.mailrs.AdminSection
import jp.golia.mailrs.addGroupMember
import jp.golia.mailrs.closeAdminRow
import jp.golia.mailrs.removeGroupMember
import jp.golia.mailrs.MailViewModel

/**
 * One group, opened.
 *
 * A group is a list with a list inside it, and the inner one is the
 * point: "Support" says nothing, "Support — lihao@golia.jp" is the
 * answer somebody came for.
 *
 * **Only email groups can be edited here.** A permission group's
 * membership decides what somebody may *do*, and granting that from a
 * phone list — no confirmation, no record of why — is not an edit worth
 * offering; what it grants is shown so the reader can see what
 * membership would mean.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupDetailScreen(detail: AdminDetail, vm: MailViewModel) {
    val theme = LocalTheme.current
    val editable = detail.section == AdminSection.EmailGroups
    var adding by remember { mutableStateOf(false) }
    var address by remember { mutableStateOf("") }

    if (adding) {
        AlertDialog(
            onDismissRequest = { adding = false },
            containerColor = theme.surface,
            title = { Text("Add member", fontSize = 16.sp, color = theme.fg) },
            text = {
                OutlinedTextField(
                    value = address,
                    onValueChange = { address = it },
                    label = { Text("Address", fontSize = 13.sp) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("field.member"),
                )
            },
            confirmButton = {
                TextButton(
                    enabled = address.isNotBlank(),
                    onClick = {
                        adding = false
                        vm.addGroupMember(address)
                        address = ""
                    },
                    modifier = Modifier.testTag("button.confirmMember"),
                ) {
                    Text("Add", color = theme.accent)
                }
            },
            dismissButton = {
                TextButton(onClick = { adding = false }) { Text("Cancel", color = theme.fgSecondary) }
            },
        )
    }

    Scaffold(
        containerColor = theme.bg,
        floatingActionButton = {
            if (editable) {
                FloatingActionButton(
                    onClick = { adding = true },
                    containerColor = theme.accent,
                    contentColor = theme.accentFg,
                    modifier = Modifier.testTag("button.addMember"),
                ) {
                    Icon(Icons.Filled.Add, contentDescription = "Add member")
                }
            }
        },
        topBar = {
            TopAppBar(
                title = { Text(detail.title, fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = theme.bg,
                    titleContentColor = theme.fg,
                ),
                navigationIcon = {
                    IconButton(
                        onClick = { vm.closeAdminRow() },
                        modifier = Modifier.testTag("button.closeGroup"),
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
            when {
                detail.loading ->
                    CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)

                detail.members.isEmpty() && detail.grants.isEmpty() ->
                    Text(
                        "Nobody is in this group.",
                        color = theme.fgMuted,
                        fontSize = 13.sp,
                        modifier = Modifier.align(Alignment.Center).padding(32.dp).testTag("group.empty"),
                    )

                else -> LazyColumn(Modifier.fillMaxSize().testTag("list.groupMembers")) {
                    if (detail.grants.isNotEmpty()) {
                        item {
                            Heading("Grants")
                        }
                        items(detail.grants) { grant ->
                            Text(
                                grant,
                                color = theme.fg,
                                fontSize = 13.sp,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(horizontal = 16.dp, vertical = 8.dp)
                                    .testTag("row.grant"),
                            )
                        }
                    }
                    item { Heading("Members") }
                    items(detail.members) { member ->
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp)
                                .testTag("row.member"),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(member, color = theme.fg, fontSize = 13.sp, modifier = Modifier.weight(1f))
                            if (editable) {
                                IconButton(
                                    onClick = { vm.removeGroupMember(member) },
                                    modifier = Modifier.testTag("button.removeMember"),
                                ) {
                                    Icon(
                                        Icons.Filled.DeleteOutline,
                                        contentDescription = "Remove $member",
                                        tint = theme.fgMuted,
                                        modifier = Modifier.size(18.dp),
                                    )
                                }
                            }
                        }
                        HorizontalDivider(color = theme.border, thickness = 0.5.dp)
                    }
                }
            }
        }
    }
}

@Composable
private fun Heading(text: String) {
    val theme = LocalTheme.current
    Column {
        Text(
            text,
            color = theme.fgMuted,
            fontSize = 12.sp,
            fontWeight = FontWeight.Medium,
            modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
        )
    }
}
