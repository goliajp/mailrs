package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Inbox
import androidx.compose.material.icons.filled.MarkEmailUnread
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.NavigationDrawerItemDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
fun MailListDrawer(current: MailList, onChoose: (MailList) -> Unit) {
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
