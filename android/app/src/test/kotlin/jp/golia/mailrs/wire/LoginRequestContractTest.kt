package jp.golia.mailrs.wire

import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The login body has the fields of the request fixture the server's own
 * tests read, so the two cannot drift apart: this app once sent
 * `username`, the server answered 422, and nothing here noticed.
 */
class LoginRequestContractTest {
    // the client's own settings, so a null is encoded the way it is sent
    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

    @Test
    fun `the login body has the server's field names`() {
        val fixture = File("../../wire-contract/requests/login.json").readText()
        val expected = Json.parseToJsonElement(fixture).jsonObject.keys
        val sent = Json.encodeToString(Wire.LoginRequest.serializer(), Wire.LoginRequest("a@b.jp", "pw"))
        assertEquals(expected, Json.parseToJsonElement(sent).jsonObject.keys)
    }

    @Test
    fun `no code means no totp_code field, not an empty one`() {
        val sent = json.encodeToString(Wire.LoginRequest.serializer(), Wire.LoginRequest("a@b.jp", "pw", null))
        assertFalse("totp_code" in json.parseToJsonElement(sent).jsonObject)
    }

    @Test
    fun `a code is sent as totp_code`() {
        val sent = json.encodeToString(Wire.LoginRequest.serializer(), Wire.LoginRequest("a@b.jp", "pw", "123456"))
        val body = json.parseToJsonElement(sent).jsonObject
        assertEquals("123456", body["totp_code"]?.jsonPrimitive?.content)
        assertEquals(setOf("address", "password", "totp_code"), body.keys)
    }

    @Test
    fun `the TOTP challenge decodes and a token answer is not one`() {
        val challenge = json.decodeFromString(Wire.TotpChallenge.serializer(), """{"requires_totp":true}""")
        assertTrue(challenge.requiresTotp)
        val token = """{"address":"a@b.jp","display_name":"A","permissions":[],"token":"t"}"""
        assertTrue(runCatching { json.decodeFromString(Wire.TotpChallenge.serializer(), token) }.isFailure)
    }
}
