package jp.golia.mailrs.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/**
 * The web client's design tokens, ported value for value — the third
 * client to hold the same numbers rather than a third interpretation of
 * them.
 *
 * mailrs's web UI runs the gds `zinc-neutral` preset, whose light and
 * dark palettes are written out as explicit hex rather than derived.
 * `ios/Mailrs/App/Theme.swift` holds the same table; the names match the
 * CSS custom properties (`--gds-fg-muted` → `fgMuted`) so a change on
 * any side is greppable from the other two.
 *
 * **Not Material You.** The first version of this screen took
 * `dynamicLightColorScheme` from the wallpaper, which is the idiomatic
 * Android thing and the wrong one here: `ios/DESIGN.md` says the three
 * clients are "one blood-line", and a mail list whose accent is
 * whatever the phone's wallpaper suggests is not that. The accent is
 * GOLIA blue on every client.
 *
 * Semantic, never literal: no screen names a colour, it names a role.
 */
data class Theme(
    val bg: Color,
    val bgSecondary: Color,
    val bgTertiary: Color,
    val surface: Color,
    val surfaceRaised: Color,
    val fg: Color,
    val fgSecondary: Color,
    val fgMuted: Color,
    val border: Color,
    val borderStrong: Color,
    val accent: Color,
    val accentFg: Color,
    val success: Color,
    val warning: Color,
    val danger: Color,
    val info: Color,
    /**
     * Which of the two this is.
     *
     * A message body needs the answer and cannot get it from the
     * colours: it decides whether mail that declares no colours of its
     * own may be painted on dark paper. Asking the system instead would
     * be a second source for the same fact.
     */
    val isDark: Boolean,
) {
    companion object {
        val Light = Theme(
            bg = Color(0xFFFAFAFA), bgSecondary = Color(0xFFF4F4F5), bgTertiary = Color(0xFFE4E4E7),
            surface = Color(0xFFFFFFFF), surfaceRaised = Color(0xFFFFFFFF),
            fg = Color(0xFF09090B), fgSecondary = Color(0xFF3F3F46), fgMuted = Color(0xFF71717A),
            border = Color(0xFFE4E4E7), borderStrong = Color(0xFFD4D4D8),
            accent = Color(0xFF3B7DDD), accentFg = Color(0xFFFFFFFF),
            success = Color(0xFF0CA678), warning = Color(0xFFE67700),
            danger = Color(0xFFE03131), info = Color(0xFF3B7DDD),
            isDark = false,
        )

        val Dark = Theme(
            bg = Color(0xFF09090B), bgSecondary = Color(0xFF0A0A0A), bgTertiary = Color(0xFF18181B),
            surface = Color(0xFF18181B), surfaceRaised = Color(0xFF27272A),
            fg = Color(0xFFFAFAFA), fgSecondary = Color(0xFFA1A1AA), fgMuted = Color(0xFF71717A),
            border = Color(0xFF27272A), borderStrong = Color(0xFF3F3F46),
            accent = Color(0xFF3B82F6), accentFg = Color(0xFFFFFFFF),
            success = Color(0xFF22C55E), warning = Color(0xFFF59E0B),
            danger = Color(0xFFEF4444), info = Color(0xFF3B82F6),
            isDark = true,
        )
    }
}

val LocalTheme = staticCompositionLocalOf { Theme.Light }

/**
 * Resolved once at the root from the effective colour scheme — no
 * screen asks which mode it is in, because asking is how one screen
 * ends up disagreeing with the next.
 */
@Composable
fun MailrsTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val theme = if (dark) Theme.Dark else Theme.Light
    // Material's own scheme is still needed for the components that
    // reach for it (text fields, ripples), so it is filled from the
    // same tokens rather than left at its defaults.
    val scheme = if (dark) {
        darkColorScheme(
            primary = theme.accent, onPrimary = theme.accentFg,
            background = theme.bg, onBackground = theme.fg,
            surface = theme.surface, onSurface = theme.fg,
            surfaceVariant = theme.bgTertiary, onSurfaceVariant = theme.fgSecondary,
            outline = theme.border, outlineVariant = theme.borderStrong,
            error = theme.danger,
        )
    } else {
        lightColorScheme(
            primary = theme.accent, onPrimary = theme.accentFg,
            background = theme.bg, onBackground = theme.fg,
            surface = theme.surface, onSurface = theme.fg,
            surfaceVariant = theme.bgTertiary, onSurfaceVariant = theme.fgSecondary,
            outline = theme.border, outlineVariant = theme.borderStrong,
            error = theme.danger,
        )
    }
    CompositionLocalProvider(LocalTheme provides theme) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
