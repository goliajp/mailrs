package jp.golia.mailrs.ui

import android.content.Intent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.platform.LocalContext
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.UiState
import jp.golia.mailrs.attachmentOpened
import jp.golia.mailrs.reportFailure
import jp.golia.mailrs.wire.Texts

/**
 * Hand a fetched file to another app: a message's attachment on the
 * thread, a draft's file in the composer.
 *
 * Handing the file on is an action, so it happens once per file and
 * then the offer is cleared. Leaving it in state would re-open the
 * same attachment on every recomposition.
 */
@Composable
fun OpenFileEffect(state: UiState, vm: MailViewModel) {
    val context = LocalContext.current
    LaunchedEffect(state.openFile) {
        val ready = state.openFile ?: return@LaunchedEffect
        val uri = androidx.core.content.FileProvider.getUriForFile(
            context,
            context.packageName + ".files",
            ready.file,
        )
        val intent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, ready.mimeType)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        // **And say so when nothing can open it.** The comment here used
        // to promise this and the code did not: `runCatching` swallowed
        // the ActivityNotFoundException and the tap did nothing at all,
        // which reads as a broken button rather than as a phone with no
        // PDF reader on it.
        val opened = runCatching { context.startActivity(intent) }.isSuccess
        vm.attachmentOpened()
        if (!opened) vm.reportFailure(Texts.get(R.string.message_no_app, ready.filename))
    }
}
