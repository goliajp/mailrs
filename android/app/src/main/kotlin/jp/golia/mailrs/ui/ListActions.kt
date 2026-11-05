package jp.golia.mailrs.ui

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.MarkEmailRead
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.PendingTriage
import jp.golia.mailrs.R
import jp.golia.mailrs.UNDO_WINDOW_MS
import jp.golia.mailrs.UiState
import jp.golia.mailrs.answerBlockSender
import jp.golia.mailrs.answerDeleteForever
import jp.golia.mailrs.applyToOpenThread
import jp.golia.mailrs.applyToSelection
import jp.golia.mailrs.askDeleteForever
import jp.golia.mailrs.dismissUndo
import jp.golia.mailrs.undo
import jp.golia.mailrs.wire.RowAction
import jp.golia.mailrs.wire.RowActions
import jp.golia.mailrs.wire.Texts
import kotlinx.coroutines.withTimeoutOrNull

/**
 * The undo snackbar.
 *
 * `SnackbarResult` is what makes this the platform's control and not a
 * lookalike: the action, the timeout and the swipe-away all come back
 * through one return value, so "the person did nothing" and "the person
 * undid it" cannot be confused — which is the whole safety of an undo.
 */
@Composable
fun UndoSnackbar(state: UiState, vm: MailViewModel, snackbars: SnackbarHostState) {
    LaunchedEffect(state.undo) {
        val pending = state.undo ?: return@LaunchedEffect
        val offersUndo = !pending.applied || pending.action.undo != null
        // **The banner lasts exactly as long as the undo does.** `Short`
        // is four seconds and the window is five, so for one second the
        // action could still be taken back and there was nothing left to
        // take it back with. One number, not two: indefinite, bounded by
        // the window itself.
        val result = withTimeoutOrNull(UNDO_WINDOW_MS) {
            snackbars.showSnackbar(
                message = doneText(pending),
                actionLabel = when (offersUndo) {
                    true -> Texts.get(R.string.common_undo)
                    false -> null
                },
                duration = SnackbarDuration.Indefinite,
            )
        }
        when (result == SnackbarResult.ActionPerformed) {
            true -> vm.undo()
            false -> vm.dismissUndo()
        }
    }
}

/** What the snackbar says once an action is done: its own words, or its name. */
private fun doneText(p: PendingTriage): String {
    p.blocked?.let { return Texts.get(R.string.block_done, it.address) }
    val id = p.action.done ?: p.action.label
    return when (p.rows.size > 1) {
        true -> Texts.get(R.string.done_count, Texts.get(id), p.rows.size)
        false -> Texts.get(id)
    }
}

/**
 * The selection's actions in the top bar: the three most used as
 * buttons, the rest in the overflow, from the same table as every other
 * surface.
 */
@Composable
fun SelectionActions(state: UiState, vm: MailViewModel, onLabel: () -> Unit) {
    val theme = LocalTheme.current
    IconButton(
        onClick = { vm.applyToSelection(RowAction.Archive) },
        modifier = Modifier.testTag("button.selectionArchive"),
    ) {
        Icon(Icons.Filled.Archive, contentDescription = stringResource(R.string.action_archive), tint = theme.fgSecondary)
    }
    IconButton(
        onClick = { vm.applyToSelection(RowAction.Read) },
        modifier = Modifier.testTag("button.selectionRead"),
    ) {
        Icon(Icons.Filled.MarkEmailRead, contentDescription = stringResource(R.string.action_read), tint = theme.fgSecondary)
    }
    IconButton(
        onClick = { vm.applyToSelection(RowAction.Star) },
        modifier = Modifier.testTag("button.selectionStar"),
    ) {
        Icon(Icons.Filled.Star, contentDescription = stringResource(R.string.action_star), tint = theme.fgSecondary)
    }
    RowActionMenu(RowActions.forSelection(state.list), tag = "button.selectionMore") { action ->
        when (action) {
            RowAction.Label -> onLabel()
            RowAction.DeleteForever -> vm.askDeleteForever(state.selected.toList())
            RowAction.Unlabel -> vm.applyToSelection(action, state.label?.id)
            else -> vm.applyToSelection(action)
        }
    }
}

/**
 * The dialogs an action can need: which label, and whether to delete for
 * good. [labelFor] says what the label goes on — the selection or the
 * open thread — and null when no label is being chosen.
 */
@Composable
fun ActionDialogs(
    state: UiState,
    vm: MailViewModel,
    labelFor: LabelTarget?,
    showForever: Boolean,
    onLabelDone: () -> Unit,
) {
    // One owner for the question: while a thread is open it asks, so a
    // two-pane screen does not show the same dialog twice.
    if (showForever && state.confirmForever.isNotEmpty()) {
        DeleteForeverDialog(state.confirmForever.size) { yes -> vm.answerDeleteForever(yes) }
    }
    val blocking = state.confirmBlock
    if (showForever && blocking != null) {
        val allowed = state.senderLists?.allowed?.contains(blocking) ?: false
        BlockSenderDialog(blocking, allowed) { yes -> vm.answerBlockSender(yes) }
    }
    if (labelFor == null) return
    LabelPickerDialog(
        labels = state.labels,
        onPick = { label ->
            onLabelDone()
            when (labelFor) {
                LabelTarget.Selection -> vm.applyToSelection(RowAction.Label, label.id)
                LabelTarget.Open -> vm.applyToOpenThread(RowAction.Label, label.id)
            }
        },
        onDismiss = onLabelDone,
    )
}

/** What a chosen label is put on. */
enum class LabelTarget { Selection, Open }
