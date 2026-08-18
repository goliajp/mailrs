package jp.golia.mailrs

import androidx.test.platform.app.InstrumentationRegistry
import org.junit.rules.ExternalResource

/**
 * System settings a test needs in place before the activity launches.
 *
 * Font scale is a global setting, and changing it while the app is up
 * recreates the activity — which leaves the compose rule holding a dead
 * one. So it is a rule ordered before that one, and it puts the setting
 * back afterwards whether the test passed or not: an enlarged font left
 * behind would meet every later test as a layout none of them was
 * written against.
 */
class SystemConfiguration : ExternalResource() {

    /**
     * Set before the activity launches. Called from a subclass's own
     * `@Before`-ordered rule, or from a test's constructor — not from
     * the test body, which is already too late.
     */
    var fontScale: String? = null

    override fun before() {
        fontScale?.let { shell("settings put system font_scale $it") }
    }

    override fun after() {
        if (fontScale != null) shell("settings put system font_scale 1.0")
    }

    private fun shell(command: String) {
        InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command).close()
    }
}
