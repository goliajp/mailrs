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

    /**
     * `WIDTHxHEIGHT`, as `wm size` takes it, with [density] in dpi.
     *
     * Resizing the display *while the app runs* is what the two-pane
     * test used to do, and it hung the suite twice: the app came back
     * idle and Compose's `waitForIdle` sat waiting for a frame the
     * reconfigured display never produced. Nothing was spinning — the
     * main thread was parked in its looper — so it looked like a
     * fluke, and it was not. Set before the activity launches, there
     * is no reconfiguration to survive.
     */
    var displaySize: String? = null
    var density: String? = null

    override fun before() {
        fontScale?.let {
            settle("font scale $it", "settings put system font_scale $it", "settings get system font_scale") {
                v -> v.trim() == it
            }
        }
        displaySize?.let {
            settle("display size $it", "wm size $it", "wm size") { v -> v.contains("Override size: $it") }
        }
        density?.let {
            settle("density $it", "wm density $it", "wm density") { v -> v.contains("Override density: $it") }
        }
    }

    /**
     * Wait for a setting to actually take, not just to be asked for.
     *
     * `wm size` returns before the display has reconfigured, so an
     * activity launched immediately afterwards can come up on the old
     * one — which is a test that quietly proves nothing. Caught by the
     * wide-screen test's own witness, which is why it has one.
     */
    /**
     * Ask, then check, and ask again if it did not take.
     *
     * Two things go wrong here and they look alike. `wm size` returns
     * before the display has reconfigured, so an activity launched
     * straight afterwards comes up on the old one — that is a wait.
     * And in a full suite, `settings put system font_scale 2.0` is
     * sometimes simply lost: ten seconds later the device still reads
     * back 1.0, though the same call in a short run takes at once. I
     * have no account of why, so the write is repeated rather than
     * explained, and the message says how many times it was asked.
     */
    private fun settle(what: String, ask: String, check: String, settled: (String) -> Boolean) {
        var last = ""
        repeat(ATTEMPTS) { attempt ->
            shell(ask)
            repeat(10) {
                last = shell(check)
                if (settled(last)) return
                Thread.sleep(100)
            }
            if (attempt == ATTEMPTS - 1) {
                error("$what never took effect after $ATTEMPTS attempts; the device said <${last.trim()}>")
            }
        }
    }

    private companion object {
        const val ATTEMPTS = 10
    }

    override fun after() {
        if (fontScale != null) shell("settings put system font_scale 1.0")
        if (displaySize != null) shell("wm size reset")
        if (density != null) shell("wm density reset")
    }

    /**
     * @return whatever the command printed, so a caller can check it.
     *
     * The stream owns the descriptor, so it is not also closed here:
     * closing it twice leaves later calls returning nothing, which
     * reads as "the setting never took effect" — and did, for one run.
     */
    private fun shell(command: String): String {
        val fd = InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command)
        return android.os.ParcelFileDescriptor.AutoCloseInputStream(fd).bufferedReader().use { it.readText() }
    }
}
