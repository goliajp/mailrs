import Foundation
import Testing

@testable import Mailrs

/// The properties that make every list behave the same way. Each one is
/// a rule some list got wrong on the web client first.
struct MailListTests {
    @Test func everyListSaysWhatItIsAndWhatEmptyMeans() {
        for list in MailList.allCases {
            #expect(!list.titleKey.isEmpty)
            #expect(!list.emptyMessageKey.isEmpty)
            #expect(!list.systemImage.isEmpty)
        }
    }

    /// Every list is translated, not only the ones someone remembered.
    ///
    /// A new list added without catalog entries shows English in the
    /// middle of a Chinese screen, and nothing else would catch it:
    /// the app still builds, and the missing key renders as itself.
    @Test func everyListIsInTheCatalog() throws {
        let app = Bundle(for: Session.self)
        let path = try #require(app.path(forResource: "zh-Hans", ofType: "lproj"),
                                "the app shipped no Chinese localization")
        let zh = try #require(Bundle(path: path))
        for list in MailList.allCases {
            #expect(zh.localizedString(forKey: list.titleKey, value: list.titleKey, table: nil)
                    != list.titleKey, "untranslated title: \(list.titleKey)")
            #expect(zh.localizedString(forKey: list.emptyMessageKey,
                                       value: list.emptyMessageKey, table: nil)
                    != list.emptyMessageKey, "untranslated empty state: \(list.emptyMessageKey)")
        }
    }

    /// Archived is cross-folder on purpose: the server drops the folder
    /// when `archived` is set, so naming one would be a contradiction
    /// rather than a refinement.
    @Test func archivedNamesNoFolder() {
        #expect(MailList.archived.axes == MailListAxes(archived: true))
    }

    /// Unread and starred are attributes, not places. Scoping them to
    /// `NonJunk` keeps spam out of a list that is not about spam;
    /// scoping them to nothing at all would drag it back in.
    @Test func attributeListsScopeToNonJunk() {
        #expect(MailList.unread.axes.folder == "NonJunk")
        #expect(MailList.unread.axes.unread == true)
        #expect(MailList.starred.axes.folder == "NonJunk")
        #expect(MailList.starred.axes.starred == true)
    }

    /// Only Archived asks for archived threads. Every other list must
    /// leave the flag off, or it silently shows the archive too.
    @Test func onlyArchivedAsksForArchived() {
        let archivedLists = MailList.allCases.filter(\.axes.archived)
        #expect(archivedLists == [.archived])
    }

    /// The folder names the server's parser actually knows —
    /// `list_threads/mod.rs` matches inbox / junk / notifications /
    /// promotions case-insensitively, plus the merged `NP` and the
    /// `NonJunk` scope.
    @Test func usesFolderNamesTheServerParses() {
        let known: Set<String> = ["Inbox", "Junk", "NP", "NonJunk"]
        for list in MailList.allCases {
            if let folder = list.axes.folder {
                #expect(known.contains(folder), "unknown folder \(folder) on \(list.title)")
            }
        }
    }
}
