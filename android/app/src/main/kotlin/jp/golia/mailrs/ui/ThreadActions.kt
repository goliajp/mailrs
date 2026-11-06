package jp.golia.mailrs.ui

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.StarBorder
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.UiState
import jp.golia.mailrs.applyToOpenThread
import jp.golia.mailrs.askDeleteForever
import jp.golia.mailrs.loadLabels
import jp.golia.mailrs.wire.RowAction
import jp.golia.mailrs.wire.RowActions
import jp.golia.mailrs.wire.Wire

/** The star in the thread's top bar: filled and lit when starred. */
@Composable
fun StarIcon(flagged: Boolean) {
    val theme = LocalTheme.current
    when (flagged) {
        true -> Icon(Icons.Filled.Star, contentDescription = stringResource(R.string.action_unstar), tint = theme.warning)
        false -> Icon(Icons.Filled.StarBorder, contentDescription = stringResource(R.string.action_star), tint = theme.fgSecondary)
    }
}

/**
 * Everything else that can be done to the open conversation, in the
 * same order as the web's menu, so a person finds an action in the same
 * place on both.
 */
@Composable
fun ThreadActionMenu(state: UiState, vm: MailViewModel, open: Wire.Conversation) {
    var labelling by rememberSaveable { mutableStateOf(false) }
    val now = System.currentTimeMillis() / 1000
    val actions = RowActions.forRow(open, state.list, now)
        // Star, unread and archive have their own buttons beside this menu.
        .filterNot { it in setOf(RowAction.Star, RowAction.Unstar, RowAction.Unread, RowAction.Archive) }
    var snoozing by rememberSaveable { mutableStateOf(false) }
    if (snoozing) {
        SnoozePickerDialog(
            onPick = { until ->
                snoozing = false
                vm.applyToOpenThread(RowAction.Snooze, until = until)
            },
            onDismiss = { snoozing = false },
        )
    }
    RowActionMenu(actions, tag = "button.threadMore") { action ->
        when (action) {
            RowAction.Label -> {
                vm.loadLabels()
                labelling = true
            }
            RowAction.Snooze -> snoozing = true
            RowAction.DeleteForever -> vm.askDeleteForever(listOf(open.threadId))
            RowAction.Unlabel -> vm.applyToOpenThread(action, state.label?.id)
            else -> vm.applyToOpenThread(action)
        }
    }
    ActionDialogs(state, vm, labelFor = labelTarget(labelling), showForever = true) { labelling = false }
}

private fun labelTarget(labelling: Boolean): LabelTarget? = when (labelling) {
    true -> LabelTarget.Open
    false -> null
}
