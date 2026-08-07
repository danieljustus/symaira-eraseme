import XCTest
@testable import SymairaEraseMe

/// Regression tests for #641: integer IDs and counts must render as plain
/// digits. SwiftUI's `Text("\(int)")` interpolation applies locale thousands
/// grouping (e.g. request ID 3221 renders as "3.221" on de_DE), so all
/// affected views wrap values via `Int.plainDigits` — the same pattern as
/// the existing `Text("PID \(String(pid))")` fix in SettingsView.
final class Issue641RegressionTests: XCTestCase {

    func testPlainDigitsRendersWithoutGroupingSeparators() {
        XCTAssertEqual(3221.plainDigits, "3221")
        XCTAssertEqual(1273.plainDigits, "1273")
        XCTAssertEqual(0.plainDigits, "0")
        XCTAssertEqual(1000000.plainDigits, "1000000")
    }

    func testPlainDigitsDiffersFromLocaleGroupedFormatting() {
        // On a grouping locale like de_DE the same value formats as "1.273";
        // plainDigits must never apply that grouping.
        let germanGrouped = 1273.formatted(.number.locale(Locale(identifier: "de_DE")))
        XCTAssertEqual(germanGrouped, "1.273", "sanity check: de_DE groups thousands")
        XCTAssertNotEqual(
            1273.plainDigits,
            germanGrouped,
            "plainDigits must not apply locale thousands grouping"
        )
    }

    func testPlainDigitsKeepsRequestIdsIntact() {
        // Request IDs like 3221 must stay verbatim ("3221", never "3.221").
        XCTAssertEqual(3221.plainDigits, "3221")
        XCTAssertFalse(3221.plainDigits.contains("."))
        XCTAssertFalse(3221.plainDigits.contains(","))
    }
}
