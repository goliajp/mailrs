package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.automirrored.filled.InsertDriveFile
import androidx.compose.material.icons.filled.PictureAsPdf
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.UiState
import jp.golia.mailrs.openAttachment
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.Wire

/**
 * The files that came with a message.
 *
 * **Inline images are not attachments.** A part with a `content_id` is
 * referenced by the body as `cid:` and is part of the message, not a
 * file offered alongside it; listing it puts "image001.png" under every
 * mail with a signature logo. The server omits the field for ordinary
 * attachments, so its absence is the test.
 *
 * **The index travels with the row.** The server identifies an
 * attachment by its position in the message, and the two here are a PDF
 * and a PNG with the same preview — a client that always asked for the
 * first would show the right name over the wrong file and look correct
 * doing it. The stub records what was actually asked for.
 */
@Composable
fun AttachmentList(uid: Int, attachments: List<Wire.Attachment>, state: UiState, vm: MailViewModel) {
    // Filtered by index so the index stays the server's, not the
    // filtered list's — dropping an inline image would otherwise shift
    // every attachment after it by one.
    val offered = attachments.withIndex().filter { it.value.contentId == null }
    if (offered.isEmpty()) return

    Column(Modifier.fillMaxWidth().padding(top = 10.dp).testTag("list.attachments")) {
        for ((index, att) in offered) {
            AttachmentRow(
                att = att,
                busy = state.openingAttachment == index,
                onOpen = { vm.openAttachment(uid, index, att) },
            )
        }
    }
}

@Composable
private fun AttachmentRow(att: Wire.Attachment, busy: Boolean, onOpen: () -> Unit) {
    val theme = LocalTheme.current
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 6.dp)
            .clip(RoundedCornerShape(8.dp))
            .background(theme.bgSecondary)
            .clickable(enabled = !busy, onClick = onOpen)
            .padding(horizontal = 10.dp, vertical = 8.dp)
            .testTag("row.attachment"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (busy) {
            CircularProgressIndicator(Modifier.size(18.dp), color = theme.accent, strokeWidth = 2.dp)
        } else {
            Icon(
                iconFor(att.contentType),
                contentDescription = null,
                tint = theme.fgSecondary,
                modifier = Modifier.size(18.dp),
            )
        }
        Column(Modifier.padding(start = 10.dp)) {
            Text(
                att.filename,
                color = theme.fg,
                fontSize = 13.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(humanSize(att.size), color = theme.fgMuted, fontSize = 11.sp)
        }
    }
}

private fun iconFor(contentType: String): ImageVector = when {
    contentType.startsWith("image/") -> Icons.Filled.Image
    contentType == "application/pdf" -> Icons.Filled.PictureAsPdf
    contentType.startsWith("text/") -> Icons.Filled.Description
    else -> Icons.AutoMirrored.Filled.InsertDriveFile
}

/**
 * A size a person reads, not a byte count.
 *
 * Decimal units, because that is what every file manager on the phone
 * shows and a mail client disagreeing with the file manager about the
 * same file is a small lie repeated often.
 */
internal fun humanSize(bytes: Long): String = when {
    bytes < 1_000 -> "$bytes B"
    bytes < 1_000_000 -> "%.0f KB".format(bytes / 1_000.0)
    bytes < 1_000_000_000 -> "%.1f MB".format(bytes / 1_000_000.0)
    else -> "%.1f GB".format(bytes / 1_000_000_000.0)
}
