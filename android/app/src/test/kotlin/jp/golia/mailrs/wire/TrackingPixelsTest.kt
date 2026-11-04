package jp.golia.mailrs.wire

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Held to `web/src/lib/tracker-samples.json` (copied to this test's resources
 * and kept the same bytes by `scripts/check-tracker-parity.sh`), which the web
 * and iPhone tests read too, so all three clients count a message alike.
 */
class TrackingPixelsTest {
    @Serializable
    private data class Sample(val name: String, val html: String, val trackers: Int)

    @Serializable
    private data class Samples(val samples: List<Sample>)

    private val json = Json { ignoreUnknownKeys = true }
    private val rules = TrackerRules.parse(File("src/main/assets/trackers.json").readText())
    private val samples =
        json.decodeFromString(Samples.serializer(), javaClass.getResource("/tracker-samples.json")!!.readText()).samples

    @Test
    fun the_shared_samples_are_read() {
        assertTrue("web/src/lib/tracker-samples.json was not read", samples.size > 10)
    }

    @Test
    fun counts_as_the_web_does() {
        val wrong = samples.filter { TrackingPixels.count(it.html, rules) != it.trackers }
            .map { "${it.name}: expected ${it.trackers}, counted ${TrackingPixels.count(it.html, rules)}" }
        assertEquals(emptyList<String>(), wrong)
    }

    @Test
    fun strips_what_it_counts() {
        for (s in samples) {
            val stripped = TrackingPixels.strip(s.html, rules)
            assertEquals(s.name, 0, TrackingPixels.count(stripped, rules))
            if (s.trackers == 0) assertEquals(s.name, s.html, stripped)
        }
    }

    @Test
    fun ships_the_web_rules() {
        assertTrue(rules.hosts.contains("mailtrack.io"))
        assertTrue(rules.paths.contains("/wf/open"))
    }
}
