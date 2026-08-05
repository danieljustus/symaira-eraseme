import XCTest
@testable import SymairaEraseMe

/// Regression tests for #620: every sidebar navigation button must expose an
/// accessible name. The SidebarButton's `.accessibilityLabel` is sourced from
/// `NavItem.accessibilityName`, so a test here fails if any nav item ever
/// loses its label source.
final class NavItemAccessibilityTests: XCTestCase {

    func testEveryNavItemHasNonEmptyAccessibilityName() {
        for item in ContentView.NavItem.allCases {
            XCTAssertFalse(
                item.accessibilityName.isEmpty,
                "NavItem \(item.rawValue) must expose a non-empty accessibility name"
            )
            XCTAssertEqual(
                item.accessibilityName, item.rawValue,
                "accessibilityName for \(item.rawValue) must match the visible label"
            )
        }
    }

    func testNavItemAccessibilityNamesMatchTheSevenSidebarEntries() {
        let expected = Set([
            "Dashboard",
            "Campaigns",
            "Requests",
            "Brokers",
            "Calendar",
            "Manual Tasks",
            "Settings",
        ])
        let actual = Set(ContentView.NavItem.allCases.map(\.accessibilityName))

        XCTAssertEqual(
            actual, expected,
            "sidebar accessibility names must exactly match the seven nav entries"
        )
        XCTAssertEqual(
            ContentView.NavItem.allCases.count, 7,
            "sidebar must expose exactly seven nav items"
        )
    }
}
