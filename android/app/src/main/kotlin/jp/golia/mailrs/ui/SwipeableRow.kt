package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.MarkEmailRead
import androidx.compose.material3.Icon
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.unit.dp

/**
 * A row you can throw away with your thumb.
 *
 * Swipe triage is Android's, not a port: the platform ships
 * `SwipeToDismissBox` and every mail app on the phone uses it, so a row
 * that only responds to a tap is a row that does not behave like the
 * others. The list had a toolbar refresh button and no gestures, which
 * is the web client's shape wearing an Android skin.
 *
 * - **Start → end** (left to right): archive. The commonest triage, on
 *   the easier direction for a right thumb.
 * - **End → start**: mark read.
 *
 * Both are undoable and neither asks: the design doc's rule is that
 * conversations get an undo rather than a confirmation, because a
 * dialog on every swipe would cost more than the mistake it prevents.
 *
 * The haptic fires when the row passes the threshold rather than when
 * it settles, so the phone confirms the gesture at the moment the
 * decision is made — which is what the platform's own lists do.
 */
@Composable
fun SwipeableConversationRow(
    onArchive: () -> Unit,
    onMarkRead: () -> Unit,
    content: @Composable () -> Unit,
) {
    val theme = LocalTheme.current
    val haptics = LocalHapticFeedback.current

    val state = rememberSwipeToDismissBoxState()

    // Observed rather than vetoed. `confirmValueChange` is deprecated —
    // "rather than relying on a callback to veto state changes, the
    // anchor set should not include disallowed anchors" — and nothing
    // here wants to veto anyway: both directions are real actions, and
    // the row leaves because the list stops containing it.
    //
    // **Keyed on the state, not on the state's value.** The obvious
    // shape, `LaunchedEffect(state.currentValue)`, cancels itself: the
    // reset below changes `currentValue`, which changes the key, which
    // kills the coroutine at that very suspension point — so the reset
    // lands and the callback after it never runs. Collecting a
    // `snapshotFlow` keeps one coroutine alive across the whole gesture.
    //
    // **And the reset comes before the callback.**
    // `rememberSwipeToDismissBoxState` saves through `rememberSaveable`
    // and `LazyColumn` keeps an item's saved state against its key, so a
    // row that is undone comes back still holding `StartToEnd`, this
    // collector sees it again, and the row archives itself a second
    // time — the person taps Undo and watches the row leave again.
    // Measured: `undo pending=true`, then a second `triage Archive t1`
    // 23 ms later. Resetting *after* the callback does not help, because
    // by then the callback has taken the row out of the list and the
    // item has been disposed with its state already saved.
    LaunchedEffect(state) {
        snapshotFlow { state.currentValue }.collect { value ->
            if (value == SwipeToDismissBoxValue.Settled) return@collect
            haptics.performHapticFeedback(HapticFeedbackType.LongPress)
            state.reset()
            when (value) {
                SwipeToDismissBoxValue.StartToEnd -> onArchive()
                SwipeToDismissBoxValue.EndToStart -> onMarkRead()
                SwipeToDismissBoxValue.Settled -> Unit
            }
        }
    }

    SwipeToDismissBox(
        state = state,
        backgroundContent = {
            val direction = state.dismissDirection
            val (colour, icon, label, alignment) = when (direction) {
                SwipeToDismissBoxValue.StartToEnd ->
                    Quad(theme.success, Icons.Filled.Archive, "Archive", Alignment.CenterStart)
                SwipeToDismissBoxValue.EndToStart ->
                    Quad(theme.accent, Icons.Filled.MarkEmailRead, "Mark read", Alignment.CenterEnd)
                SwipeToDismissBoxValue.Settled ->
                    Quad(Color.Transparent, Icons.Filled.Archive, "", Alignment.Center)
            }
            Box(
                Modifier.fillMaxSize().background(colour).padding(horizontal = 24.dp),
                contentAlignment = alignment,
            ) {
                if (label.isNotEmpty()) {
                    Icon(icon, contentDescription = label, tint = Color.White, modifier = Modifier.size(22.dp))
                }
            }
        },
        modifier = Modifier.fillMaxWidth(),
    ) {
        Box(Modifier.background(theme.bg)) { content() }
    }
}

/** Four things that travel together; Kotlin has Triple and stops there. */
private data class Quad<A, B, C, D>(val a: A, val b: B, val c: C, val d: D)

private operator fun <A, B, C, D> Quad<A, B, C, D>.component1() = a
private operator fun <A, B, C, D> Quad<A, B, C, D>.component2() = b
private operator fun <A, B, C, D> Quad<A, B, C, D>.component3() = c
private operator fun <A, B, C, D> Quad<A, B, C, D>.component4() = d
