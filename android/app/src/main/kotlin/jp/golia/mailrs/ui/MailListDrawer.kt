package jp.golia.mailrs.ui

import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.UiState
import jp.golia.mailrs.openCalendar
import jp.golia.mailrs.openDrafts
import jp.golia.mailrs.openSent
import jp.golia.mailrs.openSettings
import jp.golia.mailrs.showLabel
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.AllInbox
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.DriveFileRenameOutline
import androidx.compose.material.icons.filled.GppMaybe
import androidx.compose.material.icons.filled.Inbox
import androidx.compose.material.icons.automirrored.filled.Label
import androidx.compose.material.icons.filled.MarkEmailUnread
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.filled.Report
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Snooze
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.NavigationDrawerItemDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.R
import jp.golia.mailrs.wire.MailList
import jp.golia.mailrs.wire.WireTriage

/**
 * Which list to read, in the platform's own furniture.
 *
 * A **navigation drawer**, not a row of tabs: Material's guidance puts
 * three to five destinations in a bottom bar and everything beyond that
 * in a drawer, and there are eleven here plus the reader's labels. It is
 * also what every mail app on the phone does, so the hamburger and the
 * edge swipe already mean this to the person holding it.
 *
 * The lists are the web's, in the web's order (`MAIL_LIST_TABS` in
 * `lib/mail-lists.ts`), so a list is found in the same place on both.
 */
@Composable
fun MailListDrawer(
    current: MailList,
    currentLabel: WireTriage.Label?,
    labels: List<WireTriage.Label>,
    onDrafts: () -> Unit,
    onSent: () -> Unit,
    onCalendar: () -> Unit,
    onSettings: () -> Unit,
    onChooseLabel: (WireTriage.Label) -> Unit,
    onChoose: (MailList) -> Unit,
) {
    val theme = LocalTheme.current
    val colors = NavigationDrawerItemDefaults.colors(
        selectedContainerColor = theme.accent.copy(alpha = 0.14f),
        selectedTextColor = theme.accent,
        selectedIconColor = theme.accent,
        unselectedTextColor = theme.fgSecondary,
        unselectedIconColor = theme.fgSecondary,
    )
    val itemModifier = Modifier.padding(NavigationDrawerItemDefaults.ItemPadding)
    ModalDrawerSheet(
        drawerContainerColor = theme.bg,
        modifier = Modifier.testTag("drawer.lists"),
    ) {
        Column(Modifier.verticalScroll(rememberScrollState())) {
            Text(
                stringResource(R.string.app_name),
                color = theme.fg,
                fontSize = 20.sp,
                modifier = Modifier.padding(start = 28.dp, top = 24.dp, bottom = 16.dp),
            )
            for (list in MailList.fixed) {
                NavigationDrawerItem(
                    label = { Text(stringResource(list.title), fontSize = 14.sp) },
                    icon = { Icon(iconFor(list), contentDescription = null) },
                    selected = list == current && currentLabel == null,
                    onClick = { onChoose(list) },
                    colors = colors,
                    modifier = itemModifier.testTag("drawer.item.${list.name}"),
                )
            }
            if (labels.isNotEmpty()) {
                HorizontalDivider(color = theme.border, thickness = 0.5.dp, modifier = Modifier.padding(vertical = 8.dp))
                Text(
                    stringResource(R.string.drawer_labels),
                    color = theme.fgMuted,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(start = 28.dp, bottom = 4.dp),
                )
                for (l in labels) {
                    NavigationDrawerItem(
                        label = { Text(l.name, fontSize = 14.sp) },
                        icon = { Icon(Icons.AutoMirrored.Filled.Label, contentDescription = null) },
                        selected = currentLabel?.id == l.id,
                        onClick = { onChooseLabel(l) },
                        colors = colors,
                        modifier = itemModifier.testTag("drawer.label.${l.id}"),
                    )
                }
            }
            HorizontalDivider(color = theme.border, thickness = 0.5.dp, modifier = Modifier.padding(vertical = 8.dp))
            // Below the rule with Settings, not among the lists: neither
            // sent mail nor drafts are a place mail arrives, and the sent
            // axis is not even a mailbox.
            NavigationDrawerItem(
                label = { Text(stringResource(R.string.list_sent), fontSize = 14.sp) },
                icon = { Icon(Icons.AutoMirrored.Filled.Send, contentDescription = null) },
                selected = false,
                onClick = onSent,
                colors = colors,
                modifier = itemModifier.testTag("drawer.item.Sent"),
            )
            NavigationDrawerItem(
                label = { Text(stringResource(R.string.list_drafts), fontSize = 14.sp) },
                icon = { Icon(Icons.Filled.DriveFileRenameOutline, contentDescription = null) },
                selected = false,
                onClick = onDrafts,
                colors = colors,
                modifier = itemModifier.testTag("drawer.item.Drafts"),
            )
            NavigationDrawerItem(
                label = { Text(stringResource(R.string.calendar_title), fontSize = 14.sp) },
                icon = { Icon(Icons.Filled.CalendarMonth, contentDescription = null) },
                selected = false,
                onClick = onCalendar,
                colors = colors,
                modifier = itemModifier.testTag("drawer.item.Calendar"),
            )
            NavigationDrawerItem(
                label = { Text(stringResource(R.string.settings_title), fontSize = 14.sp) },
                icon = { Icon(Icons.Filled.Settings, contentDescription = null) },
                selected = false,
                onClick = onSettings,
                colors = colors,
                modifier = itemModifier.testTag("drawer.item.Settings"),
            )
        }
    }
}

private fun iconFor(list: MailList): ImageVector = when (list) {
    MailList.Inbox -> Icons.Filled.Inbox
    MailList.NP -> Icons.Filled.Campaign
    MailList.Unread -> Icons.Filled.MarkEmailUnread
    MailList.Starred -> Icons.Filled.Star
    MailList.Pinned -> Icons.Filled.PushPin
    MailList.Snoozed -> Icons.Filled.Snooze
    MailList.All -> Icons.Filled.AllInbox
    MailList.Archived -> Icons.Filled.Archive
    MailList.Junk -> Icons.Filled.Report
    MailList.Review -> Icons.Filled.GppMaybe
    MailList.Trash -> Icons.Filled.Delete
    MailList.Label -> Icons.AutoMirrored.Filled.Label
}

/** The drawer for the list screen: each choice closes it, then goes where it says. */
@Composable
fun ListDrawer(state: UiState, vm: MailViewModel, close: () -> Unit) {
    MailListDrawer(
        current = state.list,
        currentLabel = state.label,
        labels = state.labels,
        onChooseLabel = { close(); vm.showLabel(it) },
        onDrafts = { close(); vm.openDrafts() },
        onSent = { close(); vm.openSent() },
        onCalendar = { close(); vm.openCalendar() },
        onSettings = { close(); vm.openSettings() },
    ) { chosen ->
        close()
        vm.show(chosen)
    }
}
