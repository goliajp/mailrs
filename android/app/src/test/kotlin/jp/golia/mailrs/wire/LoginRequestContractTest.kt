package jp.golia.mailrs.wire

import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The login body has the fields of the request fixture the server's own
 * tests read, so the two cannot drift apart: this app once sent
 * `username`, the server answered 422, and nothing here noticed.
 */
class LoginRequestContractTest {
    @Test
    fun `the login body has the server's field names`() {
        val fixture = File("../../wire-contract/requests/login.json").readText()
        val expected = Json.parseToJsonElement(fixture).jsonObject.keys
        val sent = Json.encodeToString(Wire.LoginRequest.serializer(), Wire.LoginRequest("a@b.jp", "pw"))
        assertEquals(expected, Json.parseToJsonElement(sent).jsonObject.keys)
    }
}
