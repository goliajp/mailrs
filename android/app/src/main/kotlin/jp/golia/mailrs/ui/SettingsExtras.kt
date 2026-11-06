package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.R
import jp.golia.mailrs.SettingsPage
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.RemoteImages
import jp.golia.mailrs.wire.SwipeSetting
import jp.golia.mailrs.wire.Texts

/**
 * The settings the web keeps under Notifications, Security and Mail, each
 * on a page of its own, and the image setting inline. Kept out of
 * `SettingsScreen`, which is the list.
 */
@Composable
fun SettingsExtras(swipe: SwipeSetting, onSwipe: (SwipeSetting) -> Unit, onPage: (SettingsPage) -> Unit) {
    val theme = LocalTheme.current
    HorizontalDivider(color = theme.border, thickness = 0.5.dp)
    Heading(Texts.get(R.string.settings_mail_privacy))
    for (page in SettingsPage.entries) {
        TextButton(onClick = { onPage(page) }, modifier = Modifier.fillMaxWidth().testTag("settings.page.${page.name}")) {
            Text(
                Texts.get(titleOf(page)),
                color = theme.fg,
                fontSize = 14.sp,
                modifier = Modifier.fillMaxWidth().padding(start = 4.dp),
            )
        }
    }
    Heading(Texts.get(R.string.settings_images))
    ImagesChoice()
    Heading(Texts.get(R.string.swipe_title))
    SwipeSettings(swipe, onSwipe)
}

/** The page's title, for its row and its top bar. */
fun titleOf(page: SettingsPage): Int = when (page) {
    SettingsPage.Notifications -> R.string.settings_notifications
    SettingsPage.Sessions -> R.string.settings_sessions
    SettingsPage.AppPasswords -> R.string.settings_app_passwords
    SettingsPage.Vacation -> R.string.settings_vacation
    SettingsPage.Masked -> R.string.settings_masked
}

@Composable
private fun ImagesChoice() {
    val context = LocalContext.current
    val prefs = remember(context) { Prefs(context) }
    var chosen by remember { mutableStateOf(prefs.remoteImages) }
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
        RemoteImages.entries.forEachIndexed { index, option ->
            SegmentedButton(
                selected = option == chosen,
                onClick = {
                    prefs.remoteImages = option
                    chosen = option
                },
                shape = SegmentedButtonDefaults.itemShape(index, RemoteImages.entries.size),
                modifier = Modifier.testTag("images.${option.name}"),
            ) {
                Text(Texts.get(imagesLabel(option)), fontSize = 13.sp)
            }
        }
    }
}

private fun imagesLabel(option: RemoteImages): Int = when (option) {
    RemoteImages.Auto -> R.string.settings_images_auto
    RemoteImages.Click -> R.string.settings_images_click
    RemoteImages.Never -> R.string.settings_images_never
}

@Composable
private fun Heading(text: String) {
    Text(
        text,
        color = LocalTheme.current.fgMuted,
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = 16.dp, top = 20.dp, bottom = 6.dp),
    )
}
