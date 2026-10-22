package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CjkLangTest {
    @Test
    fun `a body is read in its own script`() {
        assertEquals("zh-Hans", CjkLang.of("在 Google 上宣传您的折扣和优惠"))
        assertEquals("zh-Hant", CjkLang.of("在 Google 上宣傳您的折扣和優惠，這個"))
        assertEquals("ja", CjkLang.of("楽天銀行カード（JCB）をご利用いただき"))
        assertEquals("ko", CjkLang.of("결제가 완료되었습니다"))
        assertNull(CjkLang.of("Review of your submission is complete."))
    }
}
