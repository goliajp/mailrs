import Foundation
import Testing

@testable import Mailrs

/// The renderer against the shared samples the web send path produced
/// (`web/src/lib/markdown-samples.json`, copied beside this test and kept the
/// same bytes by `scripts/check-markdown-samples.sh`).
struct MarkdownHtmlTests {
    private final class Anchor {}

    private struct Sample: Decodable {
        let name: String
        let markdown: String
        let html: String
    }

    private struct Wrap: Decodable {
        let inner: String
        let html: String
    }

    private struct Samples: Decodable {
        let samples: [Sample]
        let wrap: Wrap
    }

    private func shared() throws -> Samples {
        let url = try #require(
            Bundle(for: Anchor.self).url(forResource: "markdown-samples", withExtension: "json"),
            "markdown-samples.json is not in the test bundle"
        )
        return try JSONDecoder().decode(Samples.self, from: Data(contentsOf: url))
    }

    @Test func everySharedSampleRendersToTheWebsBytes() throws {
        let file = try shared()
        #expect(file.samples.count >= 40, "there are no samples to check")
        for sample in file.samples {
            #expect(MarkdownHtml.render(sample.markdown) == sample.html, "\(sample.name)")
        }
    }

    @Test func theEmailWrapperIsTheWebs() throws {
        let file = try shared()
        #expect(MarkdownHtml.wrap(file.wrap.inner) == file.wrap.html)
    }

    @Test func onlyABodyWithMarksOfItsOwnGetsHtml() {
        #expect(MarkdownHtml.forSend("Thanks, see https://example.com\n\nOn Mon, A wrote:\n> q") == nil)
        let html = MarkdownHtml.forSend("Hello **big**") ?? ""
        #expect(html.contains("<p>Hello <strong>big</strong></p>"))
        #expect(html.hasPrefix("<!DOCTYPE html>"))
        #expect(!(MarkdownHtml.forSend("x [y](javascript:alert(1))") ?? "").contains("javascript"))
    }
}
