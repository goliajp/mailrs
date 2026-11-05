package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Phone
import androidx.compose.material.icons.filled.RemoveCircleOutline
import androidx.compose.material.icons.filled.Security
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.triageOpenThread
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.ReasonText
import jp.golia.mailrs.wire.RowAction
import jp.golia.mailrs.wire.WireVerdict
import jp.golia.mailrs.wire.fraudVerdict

/** The reasons table and the column for the interface language. */
@Composable
internal fun rememberReasons(): Pair<ReasonText, ReasonText.Lang> {
    val context = LocalContext.current
    val language = LocalConfiguration.current.locales[0].language
    val table = remember { ReasonText.bundled(context) }
    return table to ReasonText.Lang.of(language)
}

/**
 * Why this message was held, in the four layers that decided it.
 *
 * Nothing here recomputes: it draws the verdict stored when the mail
 * arrived, with the rule-set version beside it. Almost every message has
 * none and draws nothing — an "examined, nothing found" banner on every
 * mail would make the ones that matter invisible. Same panel as the web's.
 */
@Composable
fun VerdictPanel(messageId: String, vm: MailViewModel) {
    var verdict by remember(messageId) { mutableStateOf<WireVerdict.Verdict?>(null) }
    LaunchedEffect(messageId) {
        if (messageId.isEmpty()) return@LaunchedEffect
        // a server that cannot say draws no panel rather than an alarming one
        verdict = (vm.client.fraudVerdict(messageId) as? MailrsClient.Outcome.Ok)?.value
    }
    val v = verdict ?: return
    if (v.layers.isEmpty()) {
        // mail nobody suspected, sorted by a rule, the classifier or a filter
        if (v.placement.isNotEmpty()) PlacementNote(v.placement)
        return
    }
    Panel(v, vm)
}

@Composable
private fun Panel(v: WireVerdict.Verdict, vm: MailViewModel) {
    val theme = LocalTheme.current
    val shape = RoundedCornerShape(10.dp)
    Column(
        Modifier
            .padding(top = 10.dp)
            .fillMaxWidth()
            .background(theme.danger.copy(alpha = 0.06f), shape)
            .border(1.dp, theme.fgMuted.copy(alpha = 0.3f), shape)
            .testTag("verdict.panel"),
    ) {
        Row(Modifier.padding(10.dp), verticalAlignment = Alignment.Top) {
            Icon(Icons.Filled.Security, contentDescription = null, tint = theme.danger, modifier = Modifier.size(16.dp))
            Column(Modifier.padding(start = 8.dp)) {
                Text(heldTitle(v.quarantined), color = theme.fg, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                Text(stringResource(R.string.verdict_kept), color = theme.fgMuted, fontSize = 11.sp)
            }
        }
        if (v.placement.isNotEmpty()) {
            HorizontalDivider(color = theme.fgMuted.copy(alpha = 0.2f))
            Column(Modifier.padding(horizontal = 10.dp, vertical = 6.dp)) {
                Text(stringResource(R.string.verdict_placement), color = theme.fg, fontSize = 12.sp, fontWeight = FontWeight.Medium)
                ReasonList(v.placement)
            }
        }
        HorizontalDivider(color = theme.fgMuted.copy(alpha = 0.2f))
        Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            v.layers.forEach { LayerRow(it) }
        }
        HorizontalDivider(color = theme.fgMuted.copy(alpha = 0.2f))
        Row(Modifier.padding(horizontal = 10.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.verdict_rules, v.rulesVersion),
                color = theme.fgMuted,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.weight(1f),
            )
            if (v.quarantined) {
                OutlinedButton(
                    onClick = { vm.triageOpenThread(RowAction.Release) },
                    modifier = Modifier.testTag("verdict.release"),
                ) {
                    Text(stringResource(R.string.verdict_not_fraud), fontSize = 12.sp)
                }
            }
        }
    }
}

@Composable
private fun heldTitle(quarantined: Boolean): String {
    if (quarantined) return stringResource(R.string.verdict_held)
    return stringResource(R.string.verdict_examined)
}

@Composable
private fun LayerRow(layer: WireVerdict.Layer) {
    val theme = LocalTheme.current
    val (table, lang) = rememberReasons()
    val (icon, tint) = outcomeMark(layer.outcome)
    Row(Modifier.testTag("verdict.layer.${layer.name}").semantics(mergeDescendants = true) {}) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.padding(top = 2.dp).size(14.dp))
        Column(Modifier.padding(start = 8.dp).weight(1f)) {
            Row {
                Text(layerTitle(layer.name), color = theme.fg, fontSize = 12.sp, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
                if (layer.score > 0) ScoreText(layer.score)
            }
            layerQuestion(layer.name)?.let { Text(it, color = theme.fgMuted, fontSize = 11.sp) }
            when {
                layer.reasons.isEmpty() && layer.outcome == "pass" ->
                    Text(stringResource(R.string.verdict_clean), color = theme.fgSecondary, fontSize = 11.sp)
                // the server's English detail, for a verdict older than reasons or a code this build lacks
                layer.reasons.isEmpty() || !layer.reasons.all { table.has(it.code) } ->
                    // a layer that was never asked has nothing to say, not an empty line
                    if (layer.detail.isNotEmpty()) Text(layer.detail, color = theme.fgSecondary, fontSize = 11.sp)
                else -> ReasonList(layer.reasons)
            }
        }
    }
}

// Not a tick for "not applicable": "nothing was checked" and
// "everything was fine" are different facts.
@Composable
private fun outcomeMark(outcome: String): Pair<ImageVector, Color> {
    val theme = LocalTheme.current
    return when (outcome) {
        "fail" -> Icons.Filled.Warning to theme.danger
        "pass" -> Icons.Filled.CheckCircle to theme.success
        else -> Icons.Filled.RemoveCircleOutline to theme.fgMuted
    }
}

@Composable
private fun layerTitle(name: String): String = when (name) {
    "transport" -> stringResource(R.string.verdict_layer_transport)
    "identity" -> stringResource(R.string.verdict_layer_identity)
    "provenance" -> stringResource(R.string.verdict_layer_provenance)
    "content" -> stringResource(R.string.verdict_layer_content)
    else -> name
}

@Composable
private fun layerQuestion(name: String): String? = when (name) {
    "transport" -> stringResource(R.string.verdict_question_transport)
    "identity" -> stringResource(R.string.verdict_question_identity)
    "provenance" -> stringResource(R.string.verdict_question_provenance)
    "content" -> stringResource(R.string.verdict_question_content)
    else -> null
}

/**
 * Each line beside what it added to its layer, so the layer's score is
 * the sum of the numbers the reader sees under it.
 */
@Composable
private fun ReasonList(reasons: List<WireVerdict.Reason>) {
    val theme = LocalTheme.current
    val (table, lang) = rememberReasons()
    Column {
        reasons.forEach { reason ->
            Row {
                Text(table.words(reason, lang), color = theme.fgSecondary, fontSize = 11.sp, modifier = Modifier.weight(1f))
                val score = reason.score
                if (score != null && score > 0) ScoreText(score)
            }
        }
    }
}

@Composable
private fun ScoreText(score: Double) {
    Text(scoreLabel(score), color = LocalTheme.current.fgMuted, fontSize = 10.sp, fontFamily = FontFamily.Monospace)
}

/** `+4.5`: how a layer's score and each of its lines are written. */
internal fun scoreLabel(score: Double): String = "+" + String.format(java.util.Locale.ROOT, "%.1f", score)

/** Why mail no layer found anything in was filed where it is. */
@Composable
private fun PlacementNote(reasons: List<WireVerdict.Reason>) {
    val theme = LocalTheme.current
    val shape = RoundedCornerShape(10.dp)
    Row(
        Modifier
            .padding(top = 10.dp)
            .fillMaxWidth()
            .border(1.dp, theme.fgMuted.copy(alpha = 0.3f), shape)
            .padding(10.dp)
            .testTag("verdict.placement")
            .semantics(mergeDescendants = true) {},
    ) {
        Icon(Icons.Filled.Info, contentDescription = null, tint = theme.fgMuted, modifier = Modifier.size(14.dp))
        Column(Modifier.padding(start = 8.dp)) {
            Text(stringResource(R.string.verdict_placement), color = theme.fg, fontSize = 12.sp, fontWeight = FontWeight.Medium)
            ReasonList(reasons)
        }
    }
}

/**
 * What SPF, DKIM and DMARC each said, and whether the name renders
 * reordered; under a suspicious sender only, so the reader sees which
 * check failed rather than only that one did.
 */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun SenderChecksLine(trust: String, checks: List<WireVerdict.Reason>) {
    if (trust != "suspicious" || checks.isEmpty()) return
    val (table, lang) = rememberReasons()
    FlowRow(
        Modifier.padding(top = 2.dp).testTag("sender.checks").semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        checks.forEach { check ->
            Text(table.words(check, lang), color = checkColor(check), fontSize = 11.sp)
        }
    }
}

@Composable
private fun checkColor(check: WireVerdict.Reason): Color {
    val theme = LocalTheme.current
    if (check.code == "auth-check" && check.params["result"] == "pass") return theme.fgMuted
    return theme.danger
}

/**
 * The phone numbers a message gives that its sender has not written
 * before in this conversation. A notice, not a verdict: a number proves
 * nothing about who answers it, and invoices carry them too.
 */
@Composable
fun CallbackNotice(numbers: List<String>) {
    if (numbers.isEmpty()) return
    val theme = LocalTheme.current
    Row(
        Modifier.padding(top = 8.dp).testTag("callback.notice").semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.Top,
    ) {
        Icon(Icons.Filled.Phone, contentDescription = null, tint = theme.fgMuted, modifier = Modifier.padding(top = 2.dp).size(14.dp))
        Spacer(Modifier.size(6.dp))
        Text(stringResource(R.string.read_callback, numbers.joinToString(", ")), color = theme.fgMuted, fontSize = 12.sp)
    }
}
