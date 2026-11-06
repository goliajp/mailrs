package jp.golia.mailrs

import java.io.File
import jp.golia.mailrs.wire.WireBlock
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * What the message menu's Block knows without the server: an address on
 * one list at most, never the reader's own, and request bodies that are
 * the shared fixtures the server's tests read.
 */
class BlockSenderTest {
    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

    @Test
    fun an_address_is_on_one_list_at_most() {
        val blocked = SenderLists(allowed = setOf("a@x.example")).block("a@x.example")
        assertEquals(setOf("a@x.example"), blocked.blocked)
        assertTrue(blocked.allowed.isEmpty())
        val allowed = blocked.allow("a@x.example")
        assertTrue(allowed.blocked.isEmpty())
        assertEquals(setOf("a@x.example"), allowed.allowed)
        assertTrue(allowed.unblock("a@x.example").blocked.isEmpty())
    }

    @Test
    fun the_readers_own_address_is_not_offered() {
        val state = UiState(myAddress = "Me@Golia.jp")
        assertFalse(state.canBlock("me@golia.jp"))
        assertFalse(state.canBlock(""))
        assertTrue(state.canBlock("alice@example.com"))
    }

    @Test
    fun the_bodies_are_the_shared_fixtures() {
        val block = json.encodeToString(WireBlock.BlockRequest.serializer(), WireBlock.BlockRequest("friend@example.com"))
        assertEquals(fixture("block-sender"), Json.parseToJsonElement(block))
        val undo = json.encodeToString(
            WireBlock.UnblockRequest.serializer(),
            WireBlock.UnblockRequest("friend@example.com", allow = true),
        )
        assertEquals(fixture("unblock-sender"), Json.parseToJsonElement(undo))
    }

    @Test
    fun the_answer_decodes_the_servers_field_names() {
        val answer = json.decodeFromString(
            WireBlock.BlockResponse.serializer(),
            """{"address":"a@x.example","was_allowed":true}""",
        )
        assertEquals(WireBlock.BlockResponse("a@x.example", wasAllowed = true), answer)
    }

    private fun fixture(name: String) =
        Json.parseToJsonElement(File("../../wire-contract/requests/$name.json").readText())
}
