import Testing

@testable import Mailrs

struct MailAppearanceTests {
    /// The common case: a paragraph and a link. Nothing about it wants
    /// a white rectangle in a dark thread.
    @Test func plainMailFollowsTheApp() {
        #expect(MailAppearance.followsAppTheme(html: "<p>Hello, see you Friday.</p>"))
        #expect(MailAppearance.followsAppTheme(html: "<div>Line<br>Line <a href='x'>link</a></div>"))
        #expect(MailAppearance.followsAppTheme(html: ""))
    }

    /// Anything that picks its own colours keeps its own paper —
    /// honouring dark for these is how black-on-black happens.
    @Test func styledMailKeepsItsPaper() {
        #expect(!MailAppearance.followsAppTheme(html: "<table bgcolor=\"#ffffff\"><tr><td>x</td></tr></table>"))
        #expect(!MailAppearance.followsAppTheme(html: "<div style=\"background-color:#fff\">x</div>"))
        #expect(!MailAppearance.followsAppTheme(html: "<p style=\"color:#000\">x</p>"))
        #expect(!MailAppearance.followsAppTheme(html: "<font color=\"black\">x</font>"))
        #expect(!MailAppearance.followsAppTheme(html: "<style>body{background:#eee}</style><p>x</p>"))
    }

    /// A border colour is not a text colour: it says nothing about
    /// whether the words will be legible, and treating it as a design
    /// would send ordinary mail back to the white slab.
    @Test func aBorderColourIsNotADesign() {
        #expect(MailAppearance.followsAppTheme(html: "<hr style=\"border-color:#ddd\"><p>x</p>"))
        #expect(MailAppearance.followsAppTheme(html: "<div style=\"outline-color:#ddd\">x</div>"))
    }

    /// Case is not a signal — senders write BGCOLOR as often as bgcolor.
    @Test func theScanIsCaseInsensitive() {
        #expect(!MailAppearance.followsAppTheme(html: "<TABLE BGCOLOR=\"#FFF\">x</TABLE>"))
        #expect(!MailAppearance.followsAppTheme(html: "<P STYLE=\"COLOR:#000\">x</P>"))
    }
}
