import XCTest
@testable import SymairaEraseMe

/// Regression tests for #624: the Data Directory placeholder must not drift
/// from the backend default (~/.local/share/symeraseme), and the Calendar
/// escalation row must render human-readable category labels instead of raw
/// backend identifiers.
final class Issue624RegressionTests: XCTestCase {

    func testDefaultDataDirMatchesBackendDefault() {
        XCTAssertEqual(
            ServerManager.defaultDataDir,
            "~/.local/share/symeraseme",
            "must match src/symeraseme/core/config.py data_dir default"
        )
    }

    func testDataDirectoryPlaceholderContainsDefaultPath() {
        let placeholder = "Default (\(ServerManager.defaultDataDir))"
        XCTAssertTrue(
            placeholder.contains("~/.local/share/symeraseme"),
            "placeholder must surface the backend default data directory"
        )
    }

    func testEscalationKnownIdentifiersMapToDisplayLabels() {
        XCTAssertEqual(CalendarView.escalationLabel(for: "dpa_pending"), "DPA pending")
        XCTAssertEqual(CalendarView.escalationLabel(for: "reminder"), "Reminder")
        XCTAssertEqual(CalendarView.escalationLabel(for: "none"), "No escalation")
        XCTAssertEqual(CalendarView.escalationLabel(for: ""), "No escalation")
    }

    func testEscalationUnknownIdentifiersFallBackToRawKey() {
        XCTAssertEqual(CalendarView.escalationLabel(for: "mystery_category"), "mystery_category")
    }
}
