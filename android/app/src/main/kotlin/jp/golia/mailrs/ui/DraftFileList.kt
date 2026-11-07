package jp.golia.mailrs.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.Attached
import jp.golia.mailrs.Draft
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.detach
import jp.golia.mailrs.detachStored
import jp.golia.mailrs.openDraftFile
import jp.golia.mailrs.wire.Texts

/**
 * The files in the composer. One the server keeps with the draft opens
 * through the draft file route; one still uploading, or refused with the
 * server's reason, is only on this phone and goes in the send's form.
 */
@Composable
fun DraftFileList(draft: Draft, vm: MailViewModel) {
    if (draft.stored.isEmpty() && draft.attachments.isEmpty()) return
    val theme = LocalTheme.current
    Column(Modifier.fillMaxWidth().testTag("list.draftAttachments")) {
        draft.stored.forEachIndexed { index, a ->
            FileRow(
                name = a.name,
                size = a.size,
                note = null,
                refused = false,
                onOpen = { vm.openDraftFile(a) },
                onRemove = { vm.detachStored(index) },
            )
        }
        for (a in draft.attachments) {
            FileRow(
                name = a.filename,
                size = a.size,
                note = noteOf(a),
                refused = a.refusal != null,
                onOpen = null,
                onRemove = { vm.detach(a) },
            )
        }
    }
    HorizontalDivider(color = theme.border, thickness = 0.5.dp)
}

private fun noteOf(a: Attached): String = when (val reason = a.refusal) {
    null -> Texts.get(R.string.compose_uploading)
    else -> Texts.get(R.string.compose_file_refused, reason)
}

@Composable
private fun FileRow(
    name: String,
    size: Long,
    note: String?,
    refused: Boolean,
    onOpen: (() -> Unit)?,
    onRemove: () -> Unit,
) {
    val theme = LocalTheme.current
    val opens = onOpen?.let { Modifier.clickable(onClick = it) } ?: Modifier
    Row(
        opens.testTag("row.draftAttachment").fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Filled.AttachFile, contentDescription = null, tint = theme.fgMuted, modifier = Modifier.size(15.dp))
        Column(Modifier.weight(1f).padding(start = 8.dp)) {
            Text(name, color = theme.fg, fontSize = 13.sp, maxLines = 1)
            if (note != null) {
                val tint = when (refused) {
                    true -> theme.danger
                    false -> theme.fgMuted
                }
                Text(note, color = tint, fontSize = 11.sp, modifier = Modifier.testTag("text.draftFileNote"))
            }
        }
        Text(humanSize(size), color = theme.fgMuted, fontSize = 11.sp)
        IconButton(onClick = onRemove, modifier = Modifier.testTag("button.detach")) {
            Icon(
                Icons.Filled.Close,
                contentDescription = Texts.get(R.string.compose_remove_file, name),
                tint = theme.fgMuted,
                modifier = Modifier.size(15.dp),
            )
        }
    }
}
