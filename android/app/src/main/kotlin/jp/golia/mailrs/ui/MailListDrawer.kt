package jp.golia.mailrs.ui

import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.DriveFileRenameOutline
import androidx.compose.material.icons.filled.Inbox
import androidx.compose.material.icons.filled.MarkEmailUnread
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.NavigationDrawerItemDefaults
import androidx.compose.material3.Text
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Checkbox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment

import jp.golia.mailrs.wire.FilterRow
import jp.golia.mailrs.wire.filterLabel
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.MailList

/**
 * Which list to read, in the platform's own furniture.
 *
 * A **navigation drawer**, not a row of tabs: Material's guidance puts
 * three to five destinations in a bottom bar and everything beyond that
 * in a drawer, and there are six here. It is also what every mail app on
 * the phone does, so the hamburger and the edge swipe already mean this
 * to the person holding it.
 *
 * The app had no list switch at all — one hard-coded Inbox — while the
 * web and iOS both carry all six, so Junk, Starred and Archived existed
 * on the server and nowhere a phone could reach them.
 */
@Composable
fun MailListDrawer(
    current: MailList,
    accountRows: List<FilterRow>,
    selectedAccounts: List<String>?,
    onToggleAccount: (String) -> Unit,
    onDrafts: () -> Unit,
    onSent: () -> Unit,
    onSettings: () -> Unit,
    onChoose: (MailList) -> Unit,
) {
    val theme = LocalTheme.current
    ModalDrawerSheet(
        drawerContainerColor = theme.bg,
        modifier = Modifier.testTag("drawer.lists"),
    ) {
        Text(
            "Mailrs",
            color = theme.fg,
            fontSize = 20.sp,
            modifier = Modifier.padding(start = 28.dp, top = 24.dp, bottom = 16.dp),
        )
        for (list in MailList.entries) {
            NavigationDrawerItem(
                label = { Text(list.title, fontSize = 14.sp) },
                icon = { Icon(iconFor(list), contentDescription = null) },
                selected = list == current,
                onClick = { onChoose(list) },
                colors = NavigationDrawerItemDefaults.colors(
                    selectedContainerColor = theme.accent.copy(alpha = 0.14f),
                    selectedTextColor = theme.accent,
                    selectedIconColor = theme.accent,
                    unselectedTextColor = theme.fgSecondary,
                    unselectedIconColor = theme.fgSecondary,
                ),
                modifier = Modifier
                    .padding(NavigationDrawerItemDefaults.ItemPadding)
                    .testTag("drawer.item.${list.name}"),
            )
        }
        // Nothing to narrow with one mailbox, and a filter over one
        // thing is furniture.
        if (accountRows.size > 1) {
            HorizontalDivider(
                color = theme.border,
                thickness = 0.5.dp,
                modifier = Modifier.padding(vertical = 8.dp),
            )
            Text(
                filterLabel(selectedAccounts, accountRows.map { it.id }),
                color = theme.fgMuted,
                fontSize = 12.sp,
                modifier = Modifier
                    .padding(start = 28.dp, bottom = 4.dp)
                    .testTag("drawer.accountFilter"),
            )
            for (row in accountRows) {
                val on = selectedAccounts?.contains(row.id) ?: true
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { onToggleAccount(row.id) }
                        .padding(start = 28.dp, end = 16.dp, top = 6.dp, bottom = 6.dp)
                        .testTag("drawer.account.${row.id.ifEmpty { "own" }}"),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Checkbox(checked = on, onCheckedChange = { onToggleAccount(row.id) })
                    Text(
                        row.label,
                        color = if (on) theme.fg else theme.fgMuted,
                        fontSize = 13.sp,
                    )
                }
            }
        }
        HorizontalDivider(color = theme.border, thickness = 0.5.dp, modifier = Modifier.padding(vertical = 8.dp))
        // Below the rule with Settings, not among the lists: drafts are
        // not a folder of received mail, and putting them in the same
        // group would make "Inbox, Junk, Drafts" read as three places
        // mail arrives.
        // Above Drafts, because it is the one people look for: what
        // did I send, and did it arrive. Not in the folder group for
        // the same reason Drafts is not — neither is a place mail
        // arrives, and the sent axis is not even a mailbox.
        NavigationDrawerItem(
            label = { Text("Sent", fontSize = 14.sp) },
            icon = { Icon(Icons.AutoMirrored.Filled.Send, contentDescription = null) },
            selected = false,
            onClick = onSent,
            colors = NavigationDrawerItemDefaults.colors(
                unselectedTextColor = theme.fgSecondary,
                unselectedIconColor = theme.fgSecondary,
            ),
            modifier = Modifier
                .padding(NavigationDrawerItemDefaults.ItemPadding)
                .testTag("drawer.item.Sent"),
        )
        NavigationDrawerItem(
            label = { Text("Drafts", fontSize = 14.sp) },
            icon = { Icon(Icons.Filled.DriveFileRenameOutline, contentDescription = null) },
            selected = false,
            onClick = onDrafts,
            colors = NavigationDrawerItemDefaults.colors(
                unselectedTextColor = theme.fgSecondary,
                unselectedIconColor = theme.fgSecondary,
            ),
            modifier = Modifier
                .padding(NavigationDrawerItemDefaults.ItemPadding)
                .testTag("drawer.item.Drafts"),
        )
        NavigationDrawerItem(
            label = { Text("Settings", fontSize = 14.sp) },
            icon = { Icon(Icons.Filled.Settings, contentDescription = null) },
            selected = false,
            onClick = onSettings,
            colors = NavigationDrawerItemDefaults.colors(
                unselectedTextColor = theme.fgSecondary,
                unselectedIconColor = theme.fgSecondary,
            ),
            modifier = Modifier
                .padding(NavigationDrawerItemDefaults.ItemPadding)
                .testTag("drawer.item.Settings"),
        )
    }
}

private fun iconFor(list: MailList): ImageVector = when (list) {
    MailList.Inbox -> Icons.Filled.Inbox
    MailList.NP -> Icons.Filled.Campaign
    MailList.Unread -> Icons.Filled.MarkEmailUnread
    MailList.Starred -> Icons.Filled.Star
    MailList.Junk -> Icons.Filled.Delete
    MailList.Archived -> Icons.Filled.Archive
}
