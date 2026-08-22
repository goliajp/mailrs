package jp.golia.mailrs.ui

import androidx.compose.runtime.remember
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.AdminSection
import jp.golia.mailrs.UiState
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.Prefs

/**
 * Settings: who is signed in, how it should look, and the way out.
 *
 * **Sign out moved here from the toolbar.** It was a text button next
 * to refresh — one mis-tap from losing the session, on the screen a
 * person uses most. On Android the destructive account action lives in
 * settings, at the bottom, in the colour that says so.
 *
 * Only what belongs to *this device* is offered. Signature, language
 * and time zone are the account's and are read from the server, or two
 * devices end up disagreeing about the same person.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    state: UiState,
    appearance: Prefs.Appearance,
    onAppearance: (Prefs.Appearance) -> Unit,
    onNotify: (Boolean) -> Unit,
    onClose: () -> Unit,
    onAdmin: (AdminSection) -> Unit,
    onMailAccounts: () -> Unit,
    onSignOut: () -> Unit,
) {
    val theme = LocalTheme.current
    Scaffold(
        containerColor = theme.bg,
        topBar = {
            TopAppBar(
                title = { Text("Settings", fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = theme.bg,
                    titleContentColor = theme.fg,
                ),
                navigationIcon = {
                    IconButton(onClick = onClose, modifier = Modifier.testTag("button.closeSettings")) {
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
        // Scrollable, because it is longer than a phone. It was not,
        // and the moment Administration grew to nine entries the sign
        // out at the bottom went past the edge with no way to reach it
        // — the test failed as "signing out did not return to sign-in",
        // which is what an unreachable button looks like from outside.
        Column(
            Modifier
                .padding(padding)
                .fillMaxSize()
                .background(theme.bg)
                .verticalScroll(rememberScrollState()),
        ) {
            SectionHeading("Account")
            Field("Signed in as", state.myAddress.ifBlank { "—" })
            Field("Server", state.server.ifBlank { "—" })

            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
            SectionHeading("Appearance")
            SingleChoiceSegmentedButtonRow(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            ) {
                Prefs.Appearance.entries.forEachIndexed { index, option ->
                    SegmentedButton(
                        selected = option == appearance,
                        onClick = { onAppearance(option) },
                        shape = SegmentedButtonDefaults.itemShape(index, Prefs.Appearance.entries.size),
                        modifier = Modifier.testTag("appearance.${option.name}"),
                    ) {
                        Text(option.label, fontSize = 13.sp)
                    }
                }
            }

            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
            SectionHeading("New mail")
            Row(
                Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text("Tell me when mail arrives", color = theme.fg, fontSize = 14.sp)
                    // Said plainly rather than left to be discovered.
                    // This is a check every quarter of an hour, not
                    // push, and a person who expects instant delivery
                    // has been misled by silence.
                    Text(
                        "Checked about every 15 minutes while the app is closed.",
                        color = theme.fgMuted,
                        fontSize = 12.sp,
                    )
                }
                Switch(
                    checked = state.notifyNewMail,
                    onCheckedChange = onNotify,
                    modifier = Modifier.testTag("switch.notify"),
                )
            }

            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
            // Above Administration: connecting a Gmail is something any
            // reader does for themselves, not an operator task.
            TextButton(
                onClick = onMailAccounts,
                modifier = Modifier.fillMaxWidth().testTag("settings.mailAccounts"),
            ) {
                Text(
                    "Mail accounts",
                    color = theme.fg,
                    fontSize = 14.sp,
                    modifier = Modifier.fillMaxWidth().padding(start = 4.dp),
                )
            }

            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
            SectionHeading("Administration")
            for (section in AdminSection.entries) {
                TextButton(
                    onClick = { onAdmin(section) },
                    modifier = Modifier
                        .fillMaxWidth()
                        .testTag("admin.${section.name}"),
                ) {
                    Text(
                        section.title,
                        color = theme.fg,
                        fontSize = 14.sp,
                        modifier = Modifier.fillMaxWidth().padding(start = 4.dp),
                    )
                }
            }

            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
            TextButton(
                onClick = onSignOut,
                modifier = Modifier.padding(horizontal = 8.dp, vertical = 8.dp).testTag("button.signOut"),
            ) {
                Text("Sign out", color = theme.danger, fontSize = 14.sp)
            }
        }
    }
}

@Composable
private fun SectionHeading(text: String) {
    val theme = LocalTheme.current
    Text(
        text,
        color = theme.fgMuted,
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = 16.dp, top = 20.dp, bottom = 6.dp),
    )
}

/** A label and its value, on one line, the way a settings list reads. */
@Composable
private fun Field(label: String, value: String) {
    val theme = LocalTheme.current
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = theme.fgSecondary, fontSize = 14.sp, modifier = Modifier.padding(end = 12.dp))
        Text(value, color = theme.fg, fontSize = 14.sp)
    }
}
