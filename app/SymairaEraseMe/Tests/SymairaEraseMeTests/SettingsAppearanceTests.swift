import AppKit
import SwiftUI
import SymairaTheme
import XCTest
@testable import SymairaEraseMe

final class SettingsAppearanceTests: XCTestCase {
    @MainActor
    func testProductionPickerPersistsAndAppliesToNativeWindows() async throws {
        _ = NSApplication.shared
        let previousAppearance = NSApp.appearance
        let previousHost = MCPClient.configuredHost
        let previousPort = MCPClient.configuredPort
        let previousDataDir = MCPClient.configuredDataDir
        MCPClient.configuredHost = "127.0.0.1"
        MCPClient.configuredPort = 9
        MCPClient.configuredDataDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("settings-appearance-no-credentials").path
        let suite = "com.symaira.eraseme.appearance-test.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer {
            defaults.removePersistentDomain(forName: suite)
            NSApp.appearance = previousAppearance
            MCPClient.configuredHost = previousHost
            MCPClient.configuredPort = previousPort
            MCPClient.configuredDataDir = previousDataDir
        }
        let manager = ServerManager(resourceURL: nil, executableURL: nil)
        let root = SettingsView(serverManager: manager)
            .environmentObject(manager)
            .defaultAppStorage(defaults)
        let window = NSWindow(contentViewController: NSHostingController(rootView: root))
        let otherWindow = NSWindow(contentViewController: NSHostingController(rootView: Text("Workspace")))
        window.isReleasedWhenClosed = false
        otherWindow.isReleasedWhenClosed = false
        window.setContentSize(NSSize(width: 720, height: 680))
        defer { window.close(); otherWindow.close() }
        window.contentView?.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(200))
        let control = try XCTUnwrap(findPicker(try XCTUnwrap(window.contentView)))
        for choice in [SymairaAppearance.dark, .light, .system] {
            control.selectedSegment = try XCTUnwrap(SymairaAppearance.allCases.firstIndex(of: choice))
            control.sendAction(control.action, to: control.target)
            try await Task.sleep(for: .milliseconds(100))
            XCTAssertEqual(defaults.string(forKey: "symeraseme_appearance"), choice.rawValue)
            XCTAssertEqual(NSApp.appearance?.name, choice.nativeAppearance?.name)
            XCTAssertEqual(window.effectiveAppearance.name, otherWindow.effectiveAppearance.name)
        }
        XCTAssertFalse(manager.isRunning)
    }

    @MainActor
    private func findPicker(_ view: NSView) -> NSSegmentedControl? {
        if let control = view as? NSSegmentedControl { return control }
        return view.subviews.compactMap(findPicker).first
    }
}
