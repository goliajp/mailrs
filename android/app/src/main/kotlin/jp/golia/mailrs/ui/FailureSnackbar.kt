package jp.golia.mailrs.ui

import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.UiState


/**
 * Say a failure that happened while there was already something on
 * screen.
 *
 * Every screen shows `state.error` **only when its list is empty** — a
 * conclusion in the middle of an otherwise blank page. That is right
 * for "could not load your mail" and wrong for everything else: an
 * attachment that failed to download, a bulk archive the server
 * refused, an alias that could not be deleted. Those set an error while
 * content is on screen, and until now nobody displayed it. Twenty-eight
 * places write one; four read it, all of them behind an emptiness
 * check.
 *
 * A snackbar is Android's shape for exactly this — a failure worth
 * saying and not worth a screen — and it is dismissed once said, so the
 * next thing to go wrong is said again rather than swallowed as a
 * duplicate.
 */
@Composable
fun FailureSnackbar(state: UiState, vm: MailViewModel, host: SnackbarHostState, hasContent: Boolean) {
    LaunchedEffect(state.error, hasContent) {
        val message = state.error ?: return@LaunchedEffect
        // The empty case is somebody else's: a conclusion in the middle
        // of the page says it better, and both at once says it twice.
        if (!hasContent) return@LaunchedEffect
        host.showSnackbar(message)
        vm.dismissError()
    }
}
