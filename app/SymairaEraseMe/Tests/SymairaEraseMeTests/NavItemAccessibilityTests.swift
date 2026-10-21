import XCTest
@testable import SymairaEraseMe

/// Regression tests for #620: every sidebar navigation button must expose an
/// accessible name. The shared navigation button's label is sourced from
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

    func testSidebarContainsSixDestinationsWithoutDuplicateSettings() {
        let expected = Set([
            "Dashboard",
            "Campaigns",
            "Requests",
            "Brokers",
            "Calendar",
            "Manual Tasks",
        ])
        let actual = Set(ContentView.NavItem.sidebarItems.map(\.accessibilityName))

        XCTAssertEqual(
            actual, expected,
            "sidebar accessibility names must exactly match the six product destinations"
        )
        XCTAssertEqual(
            ContentView.NavItem.sidebarItems.count, 6,
            "Settings belongs in its own native window, not the sidebar"
        )
        XCTAssertFalse(actual.contains("Settings"))
        XCTAssertEqual(ContentView.NavItem.allCases.count, 6)
    }

    /// Regression guard for #646: duplicate accessible names are as bad as
    /// missing ones for VoiceOver users — the sidebar must never present two
    /// buttons with the same announcement.
    func testSidebarAccessibilityNamesAreUnique() {
        let names = ContentView.NavItem.allCases.map(\.accessibilityName)
        XCTAssertEqual(
            Set(names).count, names.count,
            "each sidebar button needs a distinct accessible name; got \(names)"
        )
    }

    // AppKit's SymairaNavigationButton owns the single-element semantics from
    // #646. These tests pin the product-owned label sources. NativeChromeTests
    // separately mounts the production toolbar/navigation in an NSWindow;
    // in-process AX actions are not keyboard or VoiceOver acceptance.
}
