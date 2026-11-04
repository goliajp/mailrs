package jp.golia.mailrs.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.UiState
import jp.golia.mailrs.acceptShownSenders
import jp.golia.mailrs.selectSection
import jp.golia.mailrs.wire.InboxSection

/**
 * Under the Inbox's bar: the whole Inbox or one of its sections, and in New
 * a way to accept the senders shown. The web's section bar, on a phone.
 */
@Composable
fun InboxSectionBar(state: UiState, vm: MailViewModel) {
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 12.dp, vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FilterChip(
            selected = state.inboxSection == null,
            onClick = { vm.selectSection(null) },
            label = { Text(stringResource(R.string.section_all), fontSize = 13.sp) },
            modifier = Modifier.testTag("section.all"),
        )
        for (s in InboxSection.entries) {
            FilterChip(
                selected = state.inboxSection == s.wire,
                onClick = { vm.selectSection(s) },
                label = { Text(stringResource(s.label), fontSize = 13.sp) },
                modifier = Modifier.testTag("section.${s.wire}"),
            )
        }
        if (state.inboxSection == InboxSection.New.wire && state.conversations.isNotEmpty()) {
            TextButton(onClick = { vm.acceptShownSenders() }, modifier = Modifier.testTag("section.acceptAll")) {
                Text(stringResource(R.string.section_accept_all), fontSize = 13.sp)
            }
        }
    }
}
