package jp.golia.mailrs

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Only a debug build may be pointed at another server.
 *
 * `MainActivity` is exported — it has to be, it is the launcher entry
 * and the share target — so any app on the phone can start it with an
 * extra attached. One of those extras names the server this client
 * talks to, which is how the instrumented suite reaches its stub. In a
 * release build that same extra would hand somebody else's host every
 * request this app makes, including the credentials on the way in.
 *
 * Guarded by a single build flag, and nothing was watching the flag —
 * so this reads the build file. A gradle assertion rather than a
 * runtime one because the runtime cannot see the release build from
 * inside a debug test, and the flag is the whole guard.
 */
class ServerOverrideTest {

    private val buildFile = File("build.gradle.kts").takeIf { it.exists() }
        ?: File("app/build.gradle.kts")

    @Test
    fun `the release build refuses a server from an intent`() {
        val text = buildFile.readText()
        val release = text.substringAfter("release {").substringBefore("\n        }")
        assertTrue(
            "the release build no longer declares ALLOW_SERVER_OVERRIDE at all: $release",
            release.contains("ALLOW_SERVER_OVERRIDE"),
        )
        assertFalse(
            "a release build would take its server from whoever started the activity",
            release.contains("\"ALLOW_SERVER_OVERRIDE\", \"true\""),
        )
    }

    @Test
    fun `the flag is read before the override is used`() {
        // The check has to come first: reading the extra and then
        // deciding would already have handed the URL to the client.
        val source = File("src/main/kotlin/jp/golia/mailrs/MailViewModel.kt")
            .takeIf { it.exists() }
            ?: File("app/src/main/kotlin/jp/golia/mailrs/MailViewModel.kt")
        val body = source.readText().substringAfter("fun useServer(").substringBefore("\n    }")
        val guard = body.indexOf("ALLOW_SERVER_OVERRIDE")
        val use = body.indexOf("baseUrlOverride")
        assertTrue("useServer no longer checks the flag", guard >= 0)
        assertTrue("the override is applied before the flag is checked", guard < use)
    }
}
