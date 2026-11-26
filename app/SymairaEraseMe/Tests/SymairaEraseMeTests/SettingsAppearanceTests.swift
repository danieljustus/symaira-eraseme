import AppKit
import ApplicationServices
import SwiftUI
import SymairaTheme
import XCTest
@testable import SymairaEraseMe

final class SettingsAppearanceTests: XCTestCase {
    @MainActor
    func testProductionPickerPersistsAndAppliesToNativeWindows() async throws {
        // The shared picker is now a native segmented control; only an
        // AX-authorized host builds the SwiftUI tree reliably in a test window.
        guard AXIsProcessTrusted() else {
            throw XCTSkip("Driving the native segmented picker requires an AX-authorized test host")
        }
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
        // The manager publishes its own address on init; re-pin the unused port.
        MCPClient.configuredHost = "127.0.0.1"
        MCPClient.configuredPort = 9
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
        window.orderFront(nil)
        let restoreAccessibility = enableEnhancedAccessibility()
        defer { restoreAccessibility() }
        try await Task.sleep(for: .milliseconds(300))
        // CoreKit v0.25 replaced the SwiftUI segments with one native
        // NSSegmentedControl. Its AX radio cells do not implement AXPress, so
        // drive the control's own action after selecting the labelled segment.
        let picker = try XCTUnwrap(segmentedControl(in: window.contentView!), "Missing native appearance picker")
        for choice in [SymairaAppearance.dark, .light, .system] {
            let index = try XCTUnwrap(
                (0..<picker.segmentCount).first { picker.label(forSegment: $0) == choice.title },
                "Missing segment \(choice.title)")
            picker.selectedSegment = index
            XCTAssertTrue(picker.sendAction(picker.action, to: picker.target), "Cannot select \(choice.title)")
            try await Task.sleep(for: .milliseconds(150))
            XCTAssertEqual(defaults.string(forKey: "symeraseme_appearance"), choice.rawValue)
            XCTAssertEqual(NSApp.appearance?.name, choice.nativeAppearance?.name)
            XCTAssertEqual(window.effectiveAppearance.name, otherWindow.effectiveAppearance.name)
        }
        XCTAssertFalse(manager.isRunning)
    }

    @MainActor
    private func segmentedControl(in view: NSView) -> NSSegmentedControl? {
        if let control = view as? NSSegmentedControl { return control }
        for subview in view.subviews {
            if let control = segmentedControl(in: subview) { return control }
        }
        return nil
    }
}
