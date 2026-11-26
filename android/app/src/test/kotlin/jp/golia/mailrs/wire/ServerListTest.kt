package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/** The same rules as `ios/MailrsTests/ServerListTests.swift`. */
class ServerListTest {
    private val builtIn = ServerList.BUILT_IN

    private fun refusal(change: () -> ServerList): ServerList.Problem {
        try {
            change()
        } catch (refused: ServerList.Refused) {
            return refused.problem
        }
        fail("the change went through")
        throw IllegalStateException()
    }

    @Test
    fun `one server has one spelling`() {
        for (typed in listOf(
            "mail.example.com", "https://mail.example.com", "https://mail.example.com/",
            "  HTTPS://Mail.Example.com//  ",
        )) {
            assertEquals(typed, "https://mail.example.com", ServerAddress.normalise(typed))
        }
    }

    @Test
    fun `plain http, ports and paths are kept`() {
        assertEquals("http://localhost:6039", ServerAddress.normalise("http://localhost:6039"))
        assertEquals("https://192.168.1.20:8443", ServerAddress.normalise("192.168.1.20:8443"))
        assertEquals("https://example.com/mail", ServerAddress.normalise("example.com/mail/"))
        assertEquals("https://[::1]:8080", ServerAddress.normalise("[::1]:8080"))
    }

    @Test
    fun `what is not a server is refused`() {
        for (typed in listOf(
            "", "   ", "mail example.com", "ftp://mail.example.com", "https://",
            "https://user@mail.example.com", "mail.example.com?x=1", "mail.example.com#top",
            "mail..example.com", ".example.com", "example.com.", "-example.com",
            "mail.example.com:", "mail.example.com:0", "mail.example.com:70000",
            "mail.example.com:abc", "a:b:c", "[::1", "mail,example.com",
        )) {
            assertNull(typed, ServerAddress.normalise(typed))
        }
    }

    @Test
    fun `a row shows the host and never hides plain http`() {
        assertEquals("mail.example.com:8443", ServerAddress.display("https://mail.example.com:8443"))
        assertEquals("http://localhost:6039", ServerAddress.display("http://localhost:6039"))
        assertEquals("mail.example.com", ServerAddress.host("https://mail.example.com:8443/x"))
        assertEquals("[::1]", ServerAddress.host("https://[::1]:8080"))
    }

    @Test
    fun `a new list is the built-in server`() {
        val list = ServerList()
        assertEquals(listOf(builtIn), list.visible)
        assertEquals(builtIn, list.preselected)
        assertEquals("GoliaMail", builtIn.name)
        assertEquals("https://mail.golia.ai", builtIn.address)
    }

    @Test
    fun `adding keeps the name and the normalised address`() {
        val list = ServerList().add("  Office ", "Mail.Example.com/", id = "o")
        assertEquals(listOf(builtIn, SavedServer("o", "Office", "https://mail.example.com")), list.visible)
    }

    @Test
    fun `an empty name becomes the host`() {
        val list = ServerList().add("  ", "https://mail.example.com:8443")
        assertEquals("mail.example.com", list.added.single().name)
    }

    @Test
    fun `a bad or repeated address is refused`() {
        val list = ServerList().add("Office", "mail.example.com")
        assertEquals(ServerList.Problem.Address, refusal { list.add("x", "not a server") })
        assertEquals(ServerList.Problem.Duplicate, refusal { list.add("Again", "https://MAIL.example.com/") })
        assertEquals(ServerList.Problem.Duplicate, refusal { list.add("Golia", "mail.golia.ai") })
    }

    @Test
    fun `editing changes an added server only`() {
        val list = ServerList().add("Office", "mail.example.com", id = "o").add("Home", "home.example.com", id = "h")

        val edited = list.edit("o", "Work", "mx.example.com")
        assertEquals(SavedServer("o", "Work", "https://mx.example.com"), edited.visible[1])

        // its own address is not a duplicate of itself; another entry's is
        edited.edit("o", "Work 2", "mx.example.com")
        assertEquals(ServerList.Problem.Duplicate, refusal { edited.edit("o", "Work", "home.example.com") })
        assertEquals(ServerList.Problem.Address, refusal { edited.edit("o", "Work", "") })

        assertSame(edited, edited.edit(builtIn.id, "Mine", "evil.example.com"))
    }

    @Test
    fun `deleting removes an added server`() {
        val list = ServerList().add("Office", "mail.example.com", id = "o").recordSuccess("mail.example.com")
        assertEquals("o", list.lastSuccess)
        val left = list.remove("o")
        assertEquals(listOf(builtIn), left.visible)
        assertNull(left.lastSuccess)
        assertEquals(builtIn, left.preselected)
    }

    @Test
    fun `the built-in server hides and comes back`() {
        val alone = ServerList()
        // alone, it stays: the list is never empty
        assertFalse(alone.canHideBuiltIn)
        assertEquals(listOf(builtIn), alone.remove(builtIn.id).visible)

        val two = alone.add("Office", "mail.example.com", id = "o")
        assertTrue(two.canHideBuiltIn)
        val hidden = two.remove(builtIn.id)
        assertEquals(listOf("o"), hidden.visible.map { it.id })
        assertTrue(hidden.canRestoreBuiltIn)

        val back = hidden.restoreBuiltIn()
        assertEquals(listOf(builtIn.id, "o"), back.visible.map { it.id })
        assertFalse(back.canRestoreBuiltIn)
    }

    @Test
    fun `the hidden built-in returns rather than leave the list empty`() {
        val list = ServerList().add("Office", "mail.example.com", id = "o").remove(builtIn.id).remove("o")
        assertEquals(listOf(builtIn), list.visible)
        assertFalse(list.canRestoreBuiltIn)
    }

    @Test
    fun `typing the hidden built-in address brings it back`() {
        val list = ServerList().add("Office", "mail.example.com").remove(builtIn.id).add("Golia again", "mail.golia.ai")
        assertEquals(builtIn, list.visible.first())
        assertEquals(2, list.visible.size)
    }

    @Test
    fun `the last successful server is preselected`() {
        val list = ServerList().add("Office", "mail.example.com", id = "o").add("Home", "home.example.com", id = "h")
        assertEquals(builtIn, list.preselected)
        assertEquals("h", list.recordSuccess("https://home.example.com/").preselected.id)
        assertEquals("o", list.recordSuccess("mail.example.com").preselected.id)
        val golia = list.recordSuccess("mail.example.com").recordSuccess(builtIn.address)
        assertEquals(builtIn, golia.preselected)
        assertEquals(3, golia.visible.size)
    }

    @Test
    fun `a success at an unlisted address saves it`() {
        val list = ServerList().recordSuccess("https://mail.example.com:8443")
        assertEquals(2, list.visible.size)
        assertEquals("mail.example.com", list.preselected.name)
        assertEquals("https://mail.example.com:8443", list.preselected.address)
        assertEquals(list, list.recordSuccess("mail.example.com:8443"))
    }

    @Test
    fun `a success at the hidden built-in shows it again`() {
        val list = ServerList().add("Office", "mail.example.com").remove(builtIn.id).recordSuccess(builtIn.address)
        assertEquals(builtIn, list.visible.first())
        assertEquals(builtIn, list.preselected)
    }

    @Test
    fun `hiding the preselected server falls back to the first listed`() {
        val list = ServerList().add("Office", "mail.example.com", id = "o")
            .recordSuccess(builtIn.address).remove(builtIn.id)
        assertEquals("o", list.preselected.id)
    }

    @Test
    fun `an install that typed its server keeps it after the upgrade`() {
        // signed out, or never signed in: nothing was stored to carry over
        assertEquals(ServerList(), ServerList.afterUpgrade(null))

        // signed in to the built-in server: it is the remembered one
        val golia = ServerList.afterUpgrade("https://mail.golia.ai")
        assertEquals(listOf(builtIn), golia.visible)
        assertEquals(builtIn.id, golia.lastSuccess)

        // signed in to a server typed by hand: listed, named by host, chosen
        val own = ServerList.afterUpgrade("https://mail.example.com")
        assertEquals(listOf("GoliaMail", "mail.example.com"), own.visible.map { it.name })
        assertEquals("https://mail.example.com", own.preselected.address)
    }

    @Test
    fun `the list survives being written and read`() {
        val list = ServerList().add("Office", "mail.example.com", id = "o")
            .recordSuccess("mail.example.com").remove(builtIn.id)
        val json = kotlinx.serialization.json.Json
        val back = json.decodeFromString(ServerList.serializer(), json.encodeToString(ServerList.serializer(), list))
        assertEquals(list, back)
        assertEquals("Office", back.preselected.name)
        assertTrue(back.canRestoreBuiltIn)
    }
}
