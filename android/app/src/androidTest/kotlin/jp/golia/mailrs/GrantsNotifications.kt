package jp.golia.mailrs

import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Before

/**
 * Grants `POST_NOTIFICATIONS` before anything runs.
 *
 * The app asks for it after signing in, which is right on a phone and
 * fatal in a test: an unanswered permission dialog covers the activity,
 * and every Compose assertion then fails with "No compose hierarchies
 * found in the app". It took 39 of 40 tests down the first time and all
 * four of the next class the second — which is why it is a base class
 * rather than a paragraph copied into each one.
 */
abstract class GrantsNotifications {

    /**
     * And a stub that remembers nothing from the last test.
     *
     * It keeps its state across a whole run — sent messages, deleted
     * domains, verbs — so a class that does not reset inherits whatever
     * the previous one left. `AccessibilityTest` found this the hard
     * way: its inbox check timed out because an earlier test had
     * emptied the list it was waiting for.
     */
    @Before
    fun resetTheStub() {
        val stub = StubServer.base()
        runCatching {
            val c = java.net.URL("$stub/debug/reset").openConnection() as java.net.HttpURLConnection
            c.requestMethod = "POST"
            c.connectTimeout = 5_000
            c.readTimeout = 5_000
            c.inputStream.use { it.readBytes() }
        }
    }

    @Before
    fun grantNotifications() {
        if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) return
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.uiAutomation.grantRuntimePermission(
            instrumentation.targetContext.packageName,
            android.Manifest.permission.POST_NOTIFICATIONS,
        )
    }
}
