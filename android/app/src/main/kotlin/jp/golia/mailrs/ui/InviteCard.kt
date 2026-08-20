package jp.golia.mailrs.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.wire.InviteRules
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.Wire
import jp.golia.mailrs.wire.invite
import jp.golia.mailrs.wire.rsvp
import kotlinx.coroutines.launch

/**
 * A meeting, above the mail that carries it.
 *
 * The message arrives as a wall of HTML with the join link buried in
 * it; when, where and whether to say yes live in the `text/calendar`
 * part that nothing read until 2026-08-20.
 *
 * Times arrive resolved. A `TZID` is routinely a Windows name like
 * `Pacific Standard Time`, which says "Standard" while the event is in
 * daylight time, and no client-side parser can evaluate one.
 */
@Composable
fun InviteCard(uid: Int, method: String, vm: MailViewModel) {
    val theme = LocalTheme.current
    var detail by remember(uid) { mutableStateOf<Wire.MessageDetail?>(null) }
    var failure by remember(uid) { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(uid) {
        detail = (vm.client.invite(uid) as? MailrsClient.Outcome.Ok)?.value
    }

    val invite = detail?.invite ?: return
    val cancelled = method.uppercase() == "CANCEL"

    Card(
        colors = CardDefaults.cardColors(containerColor = theme.surface),
        border = BorderStroke(0.5.dp, theme.border),
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 10.dp)
            .testTag("invite.card"),
    ) {
        Column(Modifier.padding(10.dp)) {
            Text(
                InviteRules.badge(method, invite.sequence),
                color = if (cancelled) theme.danger else theme.accent,
                fontSize = 11.sp,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                invite.summary,
                color = theme.fg,
                fontSize = 15.sp,
                fontWeight = FontWeight.SemiBold,
                textDecoration = if (cancelled) TextDecoration.LineThrough else null,
                modifier = Modifier.padding(top = 2.dp).testTag("invite.summary"),
            )
            whenLine(invite)?.let {
                Text(it, color = theme.fgSecondary, fontSize = 12.sp,
                     modifier = Modifier.testTag("invite.when"))
            }
            invite.location?.takeIf { it.isNotBlank() }?.let {
                Text(it, color = theme.fgSecondary, fontSize = 12.sp, maxLines = 2)
            }
            // The way in, which is the most-used thing on a meeting
            // invitation and was missing until somebody looked at the
            // card instead of asserting about it.
            invite.joinUrl?.takeIf { !cancelled }?.let { link ->
                val uriHandler = LocalUriHandler.current
                TextButton(
                    onClick = { uriHandler.openUri(link) },
                    contentPadding = PaddingValues(0.dp),
                    modifier = Modifier.testTag("invite.join"),
                ) {
                    Text("Join the meeting", color = theme.accent, fontSize = 12.sp)
                }
            }
            invite.organizer?.let {
                Text("From ${it.cn ?: it.email}", color = theme.fgMuted, fontSize = 11.sp)
            }
            if (invite.attendees.isNotEmpty()) {
                Text(
                    InviteRules.guests(invite.attendees),
                    color = theme.fgMuted,
                    fontSize = 11.sp,
                    modifier = Modifier.testTag("invite.guests"),
                )
            }

            val answered = detail?.rsvpStatus
            if (!answered.isNullOrBlank()) {
                Text(
                    InviteRules.answered(answered),
                    color = theme.accent,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                    modifier = Modifier.testTag("invite.answered"),
                )
            } else if (InviteRules.wantsAnswer(method) && !cancelled) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    for ((label, partstat) in listOf(
                        "Yes" to "ACCEPTED",
                        "Maybe" to "TENTATIVE",
                        "No" to "DECLINED",
                    )) {
                        TextButton(
                            onClick = {
                                scope.launch {
                                    failure = null
                                    when (val r = vm.client.rsvp(uid, partstat)) {
                                        is MailrsClient.Outcome.Ok ->
                                            detail = (vm.client.invite(uid)
                                                as? MailrsClient.Outcome.Ok)
                                                ?.value
                                        // Said, not swallowed: an answer
                                        // that did not reach the
                                        // organiser leaves them waiting.
                                        is MailrsClient.Outcome.Err ->
                                            failure = r.message
                                    }
                                }
                            },
                            modifier = Modifier.testTag("invite.${partstat.lowercase()}"),
                        ) {
                            Text(label, color = theme.accent, fontSize = 12.sp)
                        }
                    }
                }
            }
            failure?.let {
                Text(it, color = theme.danger, fontSize = 11.sp)
            }
        }
    }
}

/**
 * The reader's own time, and the organiser's beside it when they
 * differ — the second is what somebody joining across an ocean checks.
 */
private fun whenLine(invite: Wire.Invite): String? {
    val local = InviteRules.localTime(invite.startsAt) ?: return null
    val zoned = invite.dtstart?.let { runCatching { it }.getOrNull() }
    val zone = zoneNameOf(zoned)
    val wall = wallClockOf(zoned)
    if (zone == null || wall == null || !InviteRules.zoneDiffers(zone)) return local
    return "$local · ${wall.drop(11).take(5)} $zone"
}

private fun zoneNameOf(dtstart: kotlinx.serialization.json.JsonElement?): String? =
    runCatching {
        dtstart?.let {
            (it as? kotlinx.serialization.json.JsonObject)
                ?.get("Zoned")
                ?.let { z -> (z as? kotlinx.serialization.json.JsonObject)?.get("tz_name") }
                ?.let { n -> (n as? kotlinx.serialization.json.JsonPrimitive)?.content }
        }
    }.getOrNull()

private fun wallClockOf(dtstart: kotlinx.serialization.json.JsonElement?): String? =
    runCatching {
        dtstart?.let {
            (it as? kotlinx.serialization.json.JsonObject)
                ?.get("Zoned")
                ?.let { z -> (z as? kotlinx.serialization.json.JsonObject)?.get("local") }
                ?.let { n -> (n as? kotlinx.serialization.json.JsonPrimitive)?.content }
        }
    }.getOrNull()
