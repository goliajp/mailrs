package jp.golia.mailrs.wire

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The shape `GET /api/accounts/external` actually answers.
 *
 * It is an **object with one key**, not a bare array — and both phones
 * deserialised it as an array for as long as the screen existed. The
 * list came back empty and the failure was swallowed, so it read as
 * "you have not connected anything". Nothing caught it because the
 * shared stub did not serve the route at all.
 */
class ExternalAccountWireTest {
    private val json = Json { ignoreUnknownKeys = true }

    /** Captured from `crates/webapi/src/handlers/external_accounts.rs::list`. */
    private val body = """
        {"accounts":[
          {"id":"acc_gmail","email":"someone@gmail.com","display_name":"Work",
           "provider":"gmail","colour":"#4285f4","state":"ok","auth":"oauth2",
           "incoming":{"host":"imap.gmail.com","port":993,"protocol":"imap","tls":"implicit"},
           "outgoing":{"host":"smtp.gmail.com","port":465,"protocol":"smtp","tls":"implicit"},
           "last_sync":1754400000,"progress":null,"last_error":null}
        ]}
    """.trimIndent()

    @Test
    fun `the list is an object with one key`() {
        val out = json.decodeFromString(ExternalAccountList.serializer(), body)
        assertEquals(1, out.accounts.size)
        assertEquals("someone@gmail.com", out.accounts[0].email)
    }

    // The mistake, stated as a fact: reading this body as an array
    // throws. Both phones did exactly that, and the screen caught the
    // throw and showed nothing.
    //
    // This pins the shape; what actually guards the client's choice of
    // deserialiser is `AccountFlowTest`, against a stub that now
    // serves the route.
    @Test
    fun `the body is not an array`() {
        val threw = runCatching {
            json.decodeFromString(
                kotlinx.serialization.builtins.ListSerializer(ExternalAccount.serializer()),
                body,
            )
        }.isFailure
        assertEquals(true, threw)
    }

    @Test
    fun `an empty answer is a list with nothing in it`() {
        val out = json.decodeFromString(ExternalAccountList.serializer(), """{"accounts":[]}""")
        assertEquals(0, out.accounts.size)
    }

    // A row written before a field existed must still decode: the
    // alternative is a screen that goes blank on an old row.
    @Test
    fun `a row from before progress existed still decodes`() {
        val out = json.decodeFromString(
            ExternalAccountList.serializer(),
            """{"accounts":[{"id":"a","email":"x@y.jp","state":"ok"}]}""",
        )
        assertEquals("x@y.jp", out.accounts[0].email)
        assertNull(out.accounts[0].progress)
    }

    /** What the sync worker writes while a full re-read is running. */
    @Test
    fun `what it is doing right now survives the wire`() {
        val out = json.decodeFromString(
            ExternalAccountList.serializer(),
            """{"accounts":[{"id":"a","email":"x@y.jp","state":"ok",
                 "progress":"reading Inbox again from the start"}]}""",
        )
        assertEquals("reading Inbox again from the start", out.accounts[0].progress)
    }
}
