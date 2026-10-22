import Testing

@testable import Mailrs

struct CjkLangTests {
    @Test func aBodyIsReadInItsOwnScript() {
        #expect(CjkLang.of("在 Google 上宣传您的折扣和优惠") == .zhHans)
        #expect(CjkLang.of("在 Google 上宣傳您的折扣和優惠，這個") == .zhHant)
        #expect(CjkLang.of("楽天銀行カード（JCB）をご利用いただき") == .ja)
        #expect(CjkLang.of("결제가 완료되었습니다") == .ko)
        #expect(CjkLang.of("Review of your submission is complete.") == nil)
    }
}
