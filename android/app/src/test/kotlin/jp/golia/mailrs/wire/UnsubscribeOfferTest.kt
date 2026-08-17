package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Which way off a list to offer, and what to call it.
 *
 * The three answers cost the reader different things, and the words on
 * the button are part of the rule rather than decoration: a reader who
 * taps "Unsubscribe" and lands in a browser has been surprised.
 */
class UnsubscribeOfferTest {

    @Test
    fun nothing_advertised_is_no_offer() {
        assertEquals(UnsubscribeOffer.None, UnsubscribeOffer.of(null))
        assertEquals(UnsubscribeOffer.None, UnsubscribeOffer.of(Wire.Unsubscribe()))
    }

    /** One-click wins whenever it is on the table: it is the free one. */
    @Test
    fun one_click_beats_a_page() {
        val offer = UnsubscribeOffer.of(
            Wire.Unsubscribe(
                oneClick = true,
                http = listOf("https://list.example/u?t=abc"),
                mailto = listOf("mailto:unsub@list.example"),
            ),
        )
        assertEquals(UnsubscribeOffer.OneClick, offer)
        assertEquals("Unsubscribe", offer.label)
    }

    /**
     * A page before an address: one tap against composing and sending a
     * message, and senders who offer both treat them the same.
     */
    @Test
    fun a_page_beats_an_address() {
        val offer = UnsubscribeOffer.of(
            Wire.Unsubscribe(
                oneClick = false,
                http = listOf("https://list.example/u?id=9"),
                mailto = listOf("mailto:unsub@list.example"),
            ),
        )
        assertEquals(UnsubscribeOffer.OpenPage("https://list.example/u?id=9"), offer)
        assertEquals("Unsubscribe on the web", offer.label)
    }

    @Test
    fun an_address_is_used_when_it_is_all_there_is() {
        val offer = UnsubscribeOffer.of(
            Wire.Unsubscribe(oneClick = false, mailto = listOf("mailto:unsub@list.example")),
        )
        assertEquals(UnsubscribeOffer.SendMail("mailto:unsub@list.example"), offer)
        assertEquals("Unsubscribe by email", offer.label)
    }

    /** An advertised-but-blank entry is not a destination. */
    @Test
    fun a_blank_entry_is_skipped() {
        val offer = UnsubscribeOffer.of(
            Wire.Unsubscribe(oneClick = false, http = listOf("", "https://list.example/u")),
        )
        assertEquals(UnsubscribeOffer.OpenPage("https://list.example/u"), offer)
    }
}
