package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Test

/** A Sent-list row has to say, in the reader's language, when a send did not simply arrive. */
class SendStatusTest {

    @Test
    fun the_states_needing_attention_read_as_words_not_wire_values() {
        TestTexts.install("values")
        assertEquals("Failed", SendStatus.text("failed"))
        assertEquals("Partly delivered", SendStatus.text("partial"))
        assertEquals("Sending", SendStatus.text("sending"))
    }

    @Test
    fun a_failed_send_reads_as_failed_in_chinese_and_japanese() {
        TestTexts.install("values-zh-rCN")
        assertEquals("发送失败", SendStatus.text("failed"))
        TestTexts.install("values-ja")
        assertEquals("送信失敗", SendStatus.text("failed"))
    }

    @Test
    fun a_partly_refused_send_is_a_warning_between_failed_and_delivered() {
        assertEquals(SendStatus.Tone.Warning, SendStatus.tone("partial"))
        assertEquals(SendStatus.Tone.Danger, SendStatus.tone("failed"))
        assertEquals(SendStatus.Tone.Success, SendStatus.tone("delivered"))
        assertEquals(SendStatus.Tone.Muted, SendStatus.tone("sending"))
    }

    /** A state this build cannot name is still shown, as the server said it. */
    @Test
    fun an_unknown_state_is_shown_as_it_came() {
        TestTexts.install("values")
        assertEquals("deferred", SendStatus.text("deferred"))
    }
}
