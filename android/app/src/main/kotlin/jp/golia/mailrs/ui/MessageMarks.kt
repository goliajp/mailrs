package jp.golia.mailrs.ui

import jp.golia.mailrs.wire.Texts
import jp.golia.mailrs.R
import androidx.compose.ui.res.stringResource
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.SenderIdentity
import jp.golia.mailrs.wire.LinkWarnings
import androidx.compose.material3.AlertDialog

@Composable
internal fun SenderTrustMark(senderTrust: String) {
    val theme = LocalTheme.current
    if (!SenderIdentity.isSuspicious(senderTrust)) return
    Icon(
        Icons.Filled.Warning,
        contentDescription = Texts.get(R.string.mail_suspicious_sender),
        tint = theme.warning,
        modifier = Modifier.padding(start = 5.dp).size(14.dp),
    )
}

/** Where it actually came from, when the name says somewhere else. */
@Composable
internal fun SenderClaimMark(sender: String) {
    val theme = LocalTheme.current
    val actual = SenderIdentity.contradictedDomain(sender) ?: return
    Text(
        actual,
        color = theme.warning,
        fontSize = 10.sp,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier
            .padding(start = 5.dp)
            .background(theme.warning.copy(alpha = 0.14f), RoundedCornerShape(5.dp))
            .padding(horizontal = 5.dp, vertical = 1.dp),
    )
}

/**
 * Asked once before a flagged link opens, with the reasons it was flagged,
 * as the web client asks.
 */
@Composable
internal fun LinkQuestion(target: LinkWarnings.Target, onDone: () -> Unit) {
    val context = LocalContext.current
    val (reasons, lang) = rememberReasons()
    val why = target.warning.reasons.joinToString(" ") { reasons.words(it, lang) }
    AlertDialog(
        onDismissRequest = onDone,
        text = { Text(stringResource(R.string.link_warning_confirm, why), modifier = Modifier.testTag("link.question")) },
        confirmButton = {
            TextButton(
                onClick = {
                    onDone()
                    runCatching {
                        context.startActivity(
                            Intent(Intent.ACTION_VIEW, android.net.Uri.parse(target.url))
                                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                        )
                    }
                },
                modifier = Modifier.testTag("link.open"),
            ) { Text(stringResource(R.string.link_warning_open)) }
        },
        dismissButton = {
            TextButton(onClick = onDone, modifier = Modifier.testTag("link.cancel")) {
                Text(stringResource(R.string.common_cancel))
            }
        },
    )
}

/** An absence, with the sentence finished. */
