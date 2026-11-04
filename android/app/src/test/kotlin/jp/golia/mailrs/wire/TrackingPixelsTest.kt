package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same cases as the iPhone's `TrackingPixelsTests`. */
class TrackingPixelsTest {
    @Test
    fun countsTheBeaconsStripTakesOut() {
        val html = """<img src="a" width="1" height="1"><img src="b" width="600"><IMG SRC="c" WIDTH=1 HEIGHT=1>"""
        assertEquals(2, TrackingPixels.count(html))
        assertEquals(0, TrackingPixels.count("<p>no images</p>"))
    }

    @Test
    fun removesBeacon() {
        val html = """<p>hi</p><img src="https://t.example/o.gif" width="1" height="1">"""
        assertEquals("<p>hi</p>", TrackingPixels.strip(html))
    }

    @Test
    fun inlineStyle() {
        val html = """<img src="https://t.example/o" style="width:1px;height:1px">x"""
        assertEquals("x", TrackingPixels.strip(html))
    }

    @Test
    fun keepsContent() {
        val html = """<img src="https://cdn.example/hero.png" width="600" height="200">"""
        assertEquals(html, TrackingPixels.strip(html))
        val bare = """<img src="cid:logo@x">"""
        assertEquals(bare, TrackingPixels.strip(bare))
    }

    @Test
    fun severalBeacons() {
        val html = "<img width=1 height=1 src=a><p>body</p><img height='1' width='1' src=b>"
        assertEquals("<p>body</p>", TrackingPixels.strip(html))
    }

    @Test
    fun noImages() {
        val html = "<p>Just text, and a &lt;img&gt; that is not a tag.</p>"
        assertEquals(html, TrackingPixels.strip(html))
    }

    @Test
    fun unclosedTag() {
        val html = """<p>a</p><img src="x" width="1" height="1""""
        assertTrue(TrackingPixels.strip(html).startsWith("<p>a</p>"))
    }
}
