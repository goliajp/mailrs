package jp.golia.mailrs.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * The app's mark, drawn rather than borrowed.
 *
 * The same artwork as the icon (`web/public/icon.svg`) and the same
 * geometry `ios/Mailrs/Features/BrandMark.swift` draws: a red field, a
 * white envelope, a pink flap, authored in a 512-unit box and scaled to
 * whatever the caller asks for — so the proportions are the icon's and
 * not an approximation of them.
 *
 * The reason it is drawn and not a tinted system glyph is written down
 * on the iOS side and applies here: a tinted `envelope` made the
 * sign-in screen blue while the launcher icon was red — the first two
 * things anyone sees, disagreeing.
 */
@Composable
fun AppMark(size: Dp = 88.dp) {
    Canvas(Modifier.size(size)) {
        val u = this.size.width / 512f

        // The icon's gradient stops, top-left to bottom-right.
        val field = Brush.linearGradient(
            colors = listOf(Color(0xFFEF4444), Color(0xFFDC2626), Color(0xFF991B1B)),
            start = Offset(0f, 0f),
            end = Offset(0f, this.size.height),
        )
        val flap = Brush.linearGradient(
            colors = listOf(Color(0xFFFDE8E8), Color(0xFFFCA5A5)),
            start = Offset(0f, 148 * u),
            end = Offset(0f, 284 * u),
        )

        drawRoundRect(brush = field, cornerRadius = CornerRadius(112 * u, 112 * u))

        drawRoundRect(
            color = Color.White.copy(alpha = 0.95f),
            topLeft = Offset(96 * u, 148 * u),
            size = Size(320 * u, 208 * u),
            cornerRadius = CornerRadius(18 * u, 18 * u),
        )

        drawPath(
            path = Path().apply {
                moveTo(96 * u, 168 * u)
                lineTo(116 * u, 148 * u)
                lineTo(396 * u, 148 * u)
                lineTo(416 * u, 168 * u)
                lineTo(256 * u, 284 * u)
                close()
            },
            brush = flap,
        )
    }
}
