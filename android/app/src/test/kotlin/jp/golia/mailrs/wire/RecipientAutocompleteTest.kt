package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Which part of a recipient line a suggestion is about. */
class RecipientAutocompleteTest {

    @Test
    fun the_token_is_what_comes_after_the_last_separator() {
        assertEquals("ali", RecipientAutocomplete.currentToken("ali"))
        assertEquals("bo", RecipientAutocomplete.currentToken("alice@example.com, bo"))
        assertEquals("bo", RecipientAutocomplete.currentToken("alice@example.com; bo"))
    }

    /** Just finished an entry: nothing is being typed. */
    @Test
    fun a_trailing_separator_leaves_no_token() {
        assertEquals("", RecipientAutocomplete.currentToken("alice@example.com, "))
    }

    /**
     * A complete address is not a question. Two characters is the floor,
     * the same one search uses.
     */
    @Test
    fun only_a_partial_name_is_worth_asking_about() {
        assertFalse(RecipientAutocomplete.shouldSuggest(""))
        assertFalse(RecipientAutocomplete.shouldSuggest("a"))
        assertTrue(RecipientAutocomplete.shouldSuggest("al"))
        assertFalse(RecipientAutocomplete.shouldSuggest("alice@example.com"))
    }

    /**
     * Completing keeps the addresses already entered and leaves a
     * separator, so the next name can be typed without punctuation.
     */
    @Test
    fun completing_replaces_only_the_token_being_typed() {
        assertEquals(
            "alice@example.com, ",
            RecipientAutocomplete.completing("ali", "Alice Smith <alice@example.com>"),
        )
        assertEquals(
            "alice@example.com, bob@example.com, ",
            RecipientAutocomplete.completing("alice@example.com, bo", "Bob <bob@example.com>"),
        )
    }
}
