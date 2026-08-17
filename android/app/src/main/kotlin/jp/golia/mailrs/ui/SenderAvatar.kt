package jp.golia.mailrs.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.wire.SenderIdentity

/**
 * The coloured-initial avatar, exactly the web's (`web/src/lib/avatar.ts`)
 * and the iOS app's: the same 16-colour palette and the same 31-multiply
 * hash over the address, so **the same correspondent wears the same
 * colour on every client**.
 *
 * The hash runs in wrapping `Int` truncated to 32 bits because that is
 * what JS's `| 0` does, and matching it is the whole point — a hash that
 * is merely similar gives the same person two colours on two phones.
 */
object SenderAvatar {

    /** tailwind-500, in the web's array order. */
    private val PALETTE = listOf(
        Color(0xFFEF4444), // red
        Color(0xFFF97316), // orange
        Color(0xFFF59E0B), // amber
        Color(0xFFEAB308), // yellow
        Color(0xFF84CC16), // lime
        Color(0xFF22C55E), // green
        Color(0xFF10B981), // emerald
        Color(0xFF14B8A6), // teal
        Color(0xFF06B6D4), // cyan
        Color(0xFF0EA5E9), // sky
        Color(0xFF3B82F6), // blue
        Color(0xFF6366F1), // indigo
        Color(0xFF8B5CF6), // violet
        Color(0xFFA855F7), // purple
        Color(0xFFD946EF), // fuchsia
        Color(0xFFEC4899), // pink
    )

    fun colorFor(sender: String): Color {
        val email = SenderIdentity.emailOf(sender)
        var hash = 0
        for (ch in email) {
            // `.toInt()` on Char is the UTF-16 code unit, which is what
            // the web's `charCodeAt` gives. Kotlin's Int is already 32
            // bits and overflows by wrapping, like JS after `| 0`.
            hash = hash * 31 + ch.code
        }
        // `abs` on Int.MIN_VALUE is itself; take the magnitude the way
        // Swift's `.magnitude` does rather than letting one address in
        // four billion index negatively.
        val magnitude = if (hash == Int.MIN_VALUE) Int.MAX_VALUE else kotlin.math.abs(hash)
        return PALETTE[magnitude % PALETTE.size]
    }

    fun initialFor(sender: String): String =
        SenderIdentity.readableName(sender).firstOrNull()?.uppercase() ?: "?"
}

/**
 * The avatar, with the unread dot on its rim.
 *
 * The dot carries its own accessibility label: colour alone is not a
 * signal, which `ios/DESIGN.md` states and both other clients obey.
 */
@Composable
fun SenderAvatarView(sender: String, size: Dp = 36.dp, unread: Boolean = false) {
    val theme = LocalTheme.current
    Box(contentAlignment = Alignment.TopEnd) {
        Box(
            Modifier
                .size(size)
                .clip(CircleShape)
                .background(SenderAvatar.colorFor(sender)),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                SenderAvatar.initialFor(sender),
                color = Color.White,
                fontSize = (size.value * 0.42f).sp,
                fontWeight = FontWeight.SemiBold,
            )
        }
        if (unread) {
            Box(
                Modifier
                    .size(11.dp)
                    .clip(CircleShape)
                    .background(theme.accent)
                    .border(2.dp, theme.bg, CircleShape)
                    .semantics { contentDescription = "Unread" }
            )
        }
    }
}
