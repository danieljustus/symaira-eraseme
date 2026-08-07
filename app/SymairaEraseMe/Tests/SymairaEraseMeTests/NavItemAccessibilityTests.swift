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

    // NOTE (no view-structure regression test for #646): the fix lives in
    // SidebarButton's body — `.accessibilityElement(children: .ignore)` on the
    // composed button content plus `.accessibilityAddTraits(.isButton)`, so the
    // button is a single accessibility element carrying the `.accessibilityLabel`
    // below. SwiftUI view modifiers are opaque — they cannot be introspected
    // from a plain XCTest without ViewInspector (which would require a
    // Package.swift dependency change, out of scope) or by launching the app and
    // querying the AX tree (UI/AX tests are out of scope for this change). The
    // tests above therefore pin the *source* of every button's accessible name;
    // the live AX tree was verified manually via the raw AXUIElement API (each
    // sidebar button exposes its name in AXDescription/AXAttributedDescription,
    // e.g. "Dashboard", matching the visible label).
}
