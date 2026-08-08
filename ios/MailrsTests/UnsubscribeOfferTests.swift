import Foundation
import Testing

@testable import Mailrs

@Suite("Unsubscribe offer")
struct UnsubscribeOfferTests {
    private func decode(_ json: String) -> Wire.Unsubscribe {
        try! JSONDecoder().decode(Wire.Unsubscribe.self, from: Data(json.utf8))
    }

    @Test("a message with no header offers nothing")
    func absentIsNone() {
        #expect(UnsubscribeOffer.of(nil) == .none)
    }

    @Test("one-click wins")
    func oneClickWins() {
        let u = decode(
            #"{"one_click": true, "http": ["https://e.example/u"], "mailto": ["mailto:a@e.example"]}"#)
        #expect(UnsubscribeOffer.of(u) == .oneClick)
    }

    /// The server side already refuses to post to plain http, so a
    /// message that only offers one is never `one_click` on the wire —
    /// but the link is still worth showing.
    @Test("without one-click, a page")
    func pageBeforeMail() {
        let u = decode(
            #"{"one_click": false, "http": ["https://e.example/u"], "mailto": ["mailto:a@e.example"]}"#)
        #expect(UnsubscribeOffer.of(u) == .openPage(URL(string: "https://e.example/u")!))
    }

    @Test("with only an address, the composer")
    func mailtoLast() {
        let u = decode(#"{"one_click": false, "mailto": ["mailto:a@e.example?subject=off"]}"#)
        #expect(UnsubscribeOffer.of(u) == .sendMail(URL(string: "mailto:a@e.example?subject=off")!))
    }

    /// Both arrays are omitted when empty, so a message with only a
    /// mailto target arrives with no `http` key at all — decoding that
    /// as a required field would drop every such offer.
    @Test("omitted arrays decode as empty")
    func omittedArrays() {
        let u = decode(#"{"one_click": false, "http": ["https://e.example/u"]}"#)
        #expect(u.mailto.isEmpty)
        #expect(u.http.count == 1)
    }

    @Test("a header with nothing usable offers nothing")
    func emptyIsNone() {
        #expect(UnsubscribeOffer.of(decode(#"{"one_click": false}"#)) == .none)
    }

    /// The three answers cost the reader different things, so they say
    /// different words. A reader who taps "Unsubscribe" and lands in
    /// Safari has been surprised.
    @Test("the label says where the tap goes")
    func labelsDiffer() {
        #expect(UnsubscribeOffer.oneClick.label != UnsubscribeOffer.openPage(URL(string: "https://x.example")!).label)
        #expect(UnsubscribeOffer.openPage(URL(string: "https://x.example")!).label
            != UnsubscribeOffer.sendMail(URL(string: "mailto:a@x.example")!).label)
    }

    @Test("availability follows the offer")
    func availability() {
        #expect(UnsubscribeOffer.oneClick.isAvailable)
        #expect(!UnsubscribeOffer.none.isAvailable)
    }
}
