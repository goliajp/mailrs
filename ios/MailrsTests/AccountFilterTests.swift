import XCTest

@testable import Mailrs

/// Narrowing the one list to some of the connected mailboxes.
final class AccountFilterTests: XCTestCase {
    private func accountsParam(_ axes: MailListAxes) -> String? {
        axes.queryItems.first { $0.name == "accounts" }?.value
    }

    /// No filter sends no parameter — one less thing for the server to
    /// walk, and the shape it reads as "every account".
    func testNoFilterSendsNothing() {
        XCTAssertNil(accountsParam(MailListAxes()))
    }

    /// The one that would show the wrong list: unticking every account
    /// and being served all of it, because an empty selection was
    /// collapsed into "no filter".
    func testUntickingEverythingIsNotTheSameAsNoFilter() {
        var axes = MailListAxes()
        axes.accounts = []
        XCTAssertEqual(accountsParam(axes), "")
    }

    func testSeveralAccountsAreCommaSeparated() {
        var axes = MailListAxes()
        axes.accounts = ["ext_a", "ext_b"]
        XCTAssertEqual(accountsParam(axes), "ext_a,ext_b")
    }

    /// This server's own mail is the empty id, so a selection holding
    /// it is not an empty selection.
    func testThisServersOwnMailCanBeNamed() {
        var axes = MailListAxes()
        axes.accounts = ["", "ext_a"]
        XCTAssertEqual(accountsParam(axes), ",ext_a")
    }
}
